//! DeepSeek 的账目：余额一枚 API Key 就够，当天的用量要控制台的 userToken。
//!
//! 为什么是它自己一个文件而不是塞进 `board_workbuddy.c`：那边是一个产品的账号接口（签到、积分、
//! 令牌有效期），这边是另一家公司的两个公开程度完全不同的接口。共用的只有「HTTPS + Bearer +
//! 一小时一次」这件事，而那件事抄一遍比抽象一层便宜。
//!
//! # 三个接口
//!
//! ```text
//! GET https://api.deepseek.com/user/balance          官方，API Key
//!      -> {"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"109.02",
//!          "granted_balance":"33.97","topped_up_balance":"75.05"}]}
//!
//! GET https://platform.deepseek.com/api/v0/users/get_user_summary      控制台，userToken
//!      -> data.biz_data.{normal_wallets,bonus_wallets,total_costs}[].{currency,balance|amount}
//!
//! GET .../api/v0/usage/by_api_key/amount?start=&end=&tz=28800          控制台，userToken
//!      -> data.biz_data.series[].buckets[].usage.{REQUEST,RESPONSE_TOKEN,
//!                                                PROMPT_CACHE_HIT_TOKEN,PROMPT_CACHE_MISS_TOKEN}
//! GET .../api/v0/usage/by_api_key/cost?start=&end=&tz=28800
//!      -> data.biz_data.data[].series[].buckets[].cost
//! ```
//!
//! 后三个是网页控制台的私有接口，抓的是浏览器登录态。它们会变，所以这一页把「没有 userToken」
//! 当成一件正常的事：余额照画，用量画破折号。
//!
//! # 一次只读一天
//!
//! `start` / `end` 是 GMT+8 的秒级时间戳，窗口最长 31 天。这一页只要今天 —— 一天的 amount 与
//! cost 各一个请求，几十 KB，读得动；三十天那份要翻几十倍。

#include <sys/stat.h>

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "esp_crt_bundle.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_http_client.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"

#include "board_hal.h"

static const char *TAG = "board_deepseek";

/** 凭据放在卡上的哪里。跟 WorkBuddy 一个卷、一个规矩。 */
static const char *DS_CONF_DIR = "/internal/AppData/DEEPSEEK";
static const char *DS_CONF_PATH = "/internal/AppData/DEEPSEEK/deepseek.conf";

/** 一枚 API Key 有多长。`sk-` 加三十二个十六进制，留一倍余量。 */
#define DS_KEY_MAX 128

/** 控制台 userToken 是一段 JWT，比 API Key 长得多。 */
#define DS_TOKEN_MAX 2048

/** 拼出来的答案。一行一个数，十个数字，一 KB 够用。 */
#define DS_ANSWER_MAX 1024

/** 一份响应。今天的用量按 model × api_key 分组，几十 KB 是常态。 */
#define DS_RESPONSE_MAX 98304

/** 网络给的耐心。 */
#define DS_TIMEOUT_MS 15000

/** worker 的栈：TLS 握手要几 KB，加上一个几十 KB 的缓冲区指针，八 KB 是这条线上一贯的数。 */
#define DS_TASK_STACK 8192

/** 一次读的分块。 */
#define DS_IO_CHUNK 2048

// =============================================================================
// 状态
// =============================================================================

static struct {
    bool used;
    char key[DS_KEY_MAX];
    char token[DS_TOKEN_MAX];
} s_account;

static struct {
    int32_t state;   /* HAL_DEEPSEEK_IDLE / _RUNNING / _DONE / _FAILED */
    int32_t fault;
} s_snap = { HAL_DEEPSEEK_IDLE, HAL_DEEPSEEK_FAULT_NONE };

static char *s_response;      /* PSRAM：几十 KB */

/* 今天每个小时的请求数，24 个。接口本来就是按小时分桶的（`buckets[].time` 相隔 3600 秒），
 * 所以这条曲线不需要多要一次请求 —— 是同一份文档里已经在的东西。 */
static long s_hours[24];
static bool s_have_hours;
static char s_answer[DS_ANSWER_MAX];
static SemaphoreHandle_t s_lock;
static bool s_inited;

/* 一个常驻的 worker，栈在 PSRAM 上。
 *
 * 每次请求新建一个任务在这里是不行的：这块板子的**内部** RAM 已经很紧（那份几十 KB 的响应缓冲
 * 在 PSRAM 上，但任务栈默认也在内部），`xTaskCreate` 直接返回失败 —— 表现就是「读一次」回 400。
 * 所以：栈从 PSRAM 里要（`CONFIG_SPIRAM_ALLOW_STACK_EXTERNAL_MEMORY`），任务建一次，之后靠
 * 一个信号量叫醒它。 */
static TaskHandle_t s_worker;
static StackType_t *s_stack;
static StaticTask_t *s_task_buffer;
static SemaphoreHandle_t s_wake;

static void lock(void)
{
    if (s_lock != NULL) {
        xSemaphoreTake(s_lock, portMAX_DELAY);
    }
}

static void unlock(void)
{
    if (s_lock != NULL) {
        xSemaphoreGive(s_lock);
    }
}

// =============================================================================
// 读一份 JSON 的几个指针
// -----------------------------------------------------------------------------
// 不写解析器：要的东西全是「拿到某个键后面的值」，而一份账单文档里同名的键不会有两处意思。
// 复杂的那一半（数组求和）在 fetch_today 里，靠的是同一个「找键」。
// =============================================================================

/** `"key"` 之后那个值的起点；找不到是 NULL。 */
static const char *value_of(const char *body, const char *key)
{
    if (body == NULL || key == NULL) {
        return NULL;
    }

    char needle[48];
    snprintf(needle, sizeof needle, "\"%s\"", key);

    const char *at = strstr(body, needle);

    if (at == NULL) {
        return NULL;
    }

    at += strlen(needle);

    while (*at == ' ' || *at == '\t' || *at == '\n' || *at == '\r' || *at == ':') {
        at++;
    }

    return at;
}

/** 一个字符串值，引号解开；不是字符串就 false。 */
static bool text_of(const char *value, char *out, size_t capacity)
{
    if (value == NULL || *value != '"' || capacity == 0) {
        return false;
    }

    value++;
    size_t used = 0;

    while (*value != '\0' && *value != '"' && used + 1 < capacity) {
        if (*value == '\\' && value[1] != '\0') {
            value++;
        }
        out[used++] = *value++;
    }

    out[used] = '\0';

    return true;
}

/** 一个数，可能是带引号的字符串（这家公司的金额全是字符串）。 */
static bool number_of(const char *value, double *out)
{
    if (value == NULL) {
        return false;
    }

    if (*value == '"') {
        value++;
    }

    char *end = NULL;
    const double parsed = strtod(value, &end);

    if (end == value) {
        return false;
    }

    *out = parsed;
    return true;
}

/** 把 `"key":` 后面所有的数加起来。数组求和就用它。 */
static double sum_of(const char *body, const char *key)
{
    if (body == NULL || key == NULL) {
        return 0.0;
    }

    char needle[48];
    snprintf(needle, sizeof needle, "\"%s\":", key);

    double total = 0.0;
    const char *at = body;

    while ((at = strstr(at, needle)) != NULL) {
        at += strlen(needle);

        double value = 0.0;

        if (number_of(at, &value)) {
            total += value;
        }
    }

    return total;
}

// =============================================================================
// HTTPS
// =============================================================================

typedef struct {
    esp_http_client_handle_t client;
    char *out;
    size_t capacity;
    size_t used;
    bool overflowed;
} ds_session_t;

static esp_err_t ds_http_event(esp_http_client_event_t *event)
{
    ds_session_t *session = (ds_session_t *)event->user_data;

    if (event->event_id == HTTP_EVENT_ON_DATA && session != NULL && event->data_len > 0) {
        const size_t room = session->capacity - 1 - session->used;
        const size_t take = ((size_t)event->data_len < room) ? (size_t)event->data_len : room;

        if (take < (size_t)event->data_len) {
            session->overflowed = true;
        }

        memcpy(session->out + session->used, event->data, take);
        session->used += take;
        session->out[session->used] = '\0';
    }

    return ESP_OK;
}

/** 一次 GET，带头，结果落在 `s_response`。 */
static int32_t ds_get(const char *host, const char *path, const char *authorization,
                      const char *extra_header, int *out_status)
{
    char url[192];
    snprintf(url, sizeof url, "https://%s%s", host, path);

    ds_session_t session = {
        .out = s_response,
        .capacity = DS_RESPONSE_MAX,
        .used = 0,
        .overflowed = false,
    };

    s_response[0] = '\0';

    const esp_http_client_config_t config = {
        .url = url,
        .method = HTTP_METHOD_GET,
        .timeout_ms = DS_TIMEOUT_MS,
        .event_handler = ds_http_event,
        .user_data = &session,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .buffer_size = DS_IO_CHUNK,
    };

    esp_http_client_handle_t client = esp_http_client_init(&config);

    if (client == NULL) {
        return HAL_DEEPSEEK_FAULT_STORAGE;
    }

    if (authorization != NULL && authorization[0] != '\0') {
        esp_http_client_set_header(client, "Authorization", authorization);
    }
    esp_http_client_set_header(client, "Accept", "application/json");

    if (extra_header != NULL) {
        esp_http_client_set_header(client, "x-app-version", extra_header);
    }

    int status = 0;
    esp_err_t err = esp_http_client_perform(client);

    if (err != ESP_OK && status == 0) {
        status = esp_http_client_get_status_code(client);
    }

    status = esp_http_client_get_status_code(client);
    *out_status = status;

    esp_http_client_cleanup(client);

    if (err != ESP_OK && status == 0) {
        return HAL_DEEPSEEK_FAULT_UNREACHABLE;
    }

    if (status == 401 || status == 403) {
        return HAL_DEEPSEEK_FAULT_AUTH;
    }

    if (status < 200 || status >= 300) {
        return HAL_DEEPSEEK_FAULT_HTTP;
    }

    return HAL_DEEPSEEK_FAULT_NONE;
}

// =============================================================================
// 读
// =============================================================================

static void set_fault(int32_t fault, const char *message)
{
    const int made = snprintf(s_answer, DS_ANSWER_MAX, "error=%s\nmessage=%s\n",
                              hal_deepseek_fault_code(fault), message != NULL ? message : "");

    lock();
    s_snap.state = HAL_DEEPSEEK_FAILED;
    s_snap.fault = fault;
    unlock();

    (void)made;
}

/** 官方余额。**/
static bool read_balance(double *total, double *granted, double *topped, char *currency,
                         size_t capacity, bool *available)
{
    char authorization[DS_KEY_MAX + 16];
    snprintf(authorization, sizeof authorization, "Bearer %s", s_account.key);

    int status = 0;
    const int32_t fault = ds_get("api.deepseek.com", "/user/balance", authorization, NULL, &status);

    if (fault != HAL_DEEPSEEK_FAULT_NONE) {
        set_fault(fault, status == 0 ? "balance: no answer" : "balance: refused");
        return false;
    }

    const char *body = s_response;

    /* 这家公司把拒绝也放在 200 里：`{"error":{"message":"..."}}`。 */
    const char *refusal = value_of(body, "message");

    if (value_of(body, "balance_infos") == NULL) {
        char said[192] = "";
        text_of(refusal, said, sizeof said);
        set_fault(HAL_DEEPSEEK_FAULT_API, said[0] != '\0' ? said : "balance: no balance_infos");
        return false;
    }

    *available = value_of(body, "is_available") != NULL &&
                 strncmp(value_of(body, "is_available"), "true", 4) == 0;

    const char *info = value_of(body, "balance_infos");
    if (!text_of(value_of(info, "currency"), currency, capacity)) {
        snprintf(currency, capacity, "CNY");
    }

    number_of(value_of(info, "total_balance"), total);
    number_of(value_of(info, "granted_balance"), granted);
    number_of(value_of(info, "topped_up_balance"), topped);

    return true;
}

/** 控制台把拒绝也放在 200 里：`{"code":40003,"msg":"Authorization Failed (invalid token)"}`。
 *
 * 这种响应没有 `biz_data`，光看「没有 biz_data」分不出「这枚不作数了」和「接口改了」——
 * 而这两件事对人的意思完全不同，所以先看 `code`。 */
static bool platform_refused(const char *body)
{
    if (body == NULL || value_of(body, "biz_data") != NULL) {
        return false;
    }

    double code = 0.0;

    if (!number_of(value_of(body, "code"), &code)) {
        return false;
    }

    return code != 0.0;
}

/** 控制台那份摘要：累计消费。也顺手给余额（它和官方接口说的是同一件事）。
 *
 * 交回故障码而不是布尔：`AUTH` 的意思是「这枚 userToken 不作数了」，那是一句要去网页重新拿一枚
 * 的话，和「网络没通」不是同一句。 */
static int32_t read_summary(double *total_cost)
{
    char authorization[DS_TOKEN_MAX + 16];
    snprintf(authorization, sizeof authorization, "Bearer %s", s_account.token);

    int status = 0;
    const int32_t fault =
        ds_get("platform.deepseek.com", "/api/v0/users/get_user_summary", authorization, "1.0.0",
               &status);

    if (fault != HAL_DEEPSEEK_FAULT_NONE) {
        return fault;
    }

    const char *body = s_response;

    if (value_of(body, "biz_data") == NULL) {
        return platform_refused(body) ? HAL_DEEPSEEK_FAULT_AUTH : HAL_DEEPSEEK_FAULT_API;
    }

    /* 只要累计消费。
     *
     * 这份摘要里也有钱包余额（`normal_wallets` / `bonus_wallets`），但余额的权威是官方那个
     * `/user/balance`：两处口径差一分钱就会让页面在两次读取之间跳一下，而这里要的只是那个
     * 官方接口不肯说的数 —— 累计消费。 */
    *total_cost = sum_of(body, "amount");

    return HAL_DEEPSEEK_FAULT_NONE;
}

/** 今天：请求数、token、消费。 */
static int32_t read_today(long long *requests, unsigned long long *tokens, double *cost)
{
    char authorization[DS_TOKEN_MAX + 16];
    snprintf(authorization, sizeof authorization, "Bearer %s", s_account.token);

    /* GMT+8 的今天，零点到明天零点。 */
    const time_t now = time(NULL) + 8 * 3600;
    struct tm tm;

    if (gmtime_r(&now, &tm) == NULL) {
        return HAL_DEEPSEEK_FAULT_FAILED;
    }

    const time_t midnight = ((now / 86400) * 86400) - 8 * 3600;

    char amount_path[192];
    snprintf(amount_path, sizeof amount_path,
             "/api/v0/usage/by_api_key/amount?start=%lld&end=%lld&tz=28800",
             (long long)midnight, (long long)(midnight + 86400));

    int status = 0;
    const int32_t amount_fault =
        ds_get("platform.deepseek.com", amount_path, authorization, "1.0.0", &status);

    if (amount_fault != HAL_DEEPSEEK_FAULT_NONE) {
        return amount_fault;
    }

    if (platform_refused(s_response)) {
        return HAL_DEEPSEEK_FAULT_AUTH;
    }

    /* 数组求和：同一个键在 `series[].buckets[].usage` 里出现若干次，全加起来就是这一天的量。 */
    /* 按小时摊开：每个桶先给 `time`，桶里的用量紧随其后，所以「找 time、再找它后面的
     * REQUEST」就是这一小时的值。同一个小时可能在好几个 series（每个 model 一条）里出现，
     * 累加就是这一小时的总数。 */
    memset(s_hours, 0, sizeof s_hours);

    for (const char *at = strstr(s_response, "\"time\":"); at != NULL;
         at = strstr(at + 7, "\"time\":")) {
        double stamp = 0.0;

        if (!number_of(at + 7, &stamp)) {
            continue;
        }

        const long index = (long)((stamp - (double)midnight) / 3600.0);
        const char *count_at = strstr(at, "\"REQUEST\":");

        if (count_at == NULL || index < 0 || index >= 24) {
            continue;
        }

        double count = 0.0;

        if (number_of(count_at + 10, &count)) {
            s_hours[index] += (long)count;
        }
    }

    s_have_hours = true;

    const double requests_sum = sum_of(s_response, "REQUEST");
    const double response = sum_of(s_response, "RESPONSE_TOKEN");
    const double hit = sum_of(s_response, "PROMPT_CACHE_HIT_TOKEN");
    const double miss = sum_of(s_response, "PROMPT_CACHE_MISS_TOKEN");

    *requests = (long long)requests_sum;
    *tokens = (unsigned long long)(response + hit + miss);

    char cost_path[192];
    snprintf(cost_path, sizeof cost_path,
             "/api/v0/usage/by_api_key/cost?start=%lld&end=%lld&tz=28800", (long long)midnight,
             (long long)(midnight + 86400));

    if (ds_get("platform.deepseek.com", cost_path, authorization, "1.0.0", &status) ==
        HAL_DEEPSEEK_FAULT_NONE) {
        *cost = sum_of(s_response, "cost");
    } else {
        *cost = 0.0;
    }

    return HAL_DEEPSEEK_FAULT_NONE;
}

/** 一份说完的答案。 */
static void render(bool available, const char *currency, double total, double granted,
                   double topped, bool have_summary, double total_cost, bool have_today,
                   long long requests, unsigned long long tokens, double cost,
                   const char *token_state)
{
    /* 24 个整数，逗号分开。答案是一段纯文本，这一点比 JSON 省事得多。 */
    char hours_text[192] = "";
    int hours_used = 0;

    if (have_today && s_have_hours) {
        for (int hour = 0; hour < 24; hour++) {
            const int made = snprintf(hours_text + hours_used,
                                      sizeof hours_text - (size_t)hours_used, "%s%ld",
                                      hour == 0 ? "" : ",", s_hours[hour]);

            if (made <= 0 || (size_t)(hours_used + made) >= sizeof hours_text) {
                break;
            }

            hours_used += made;
        }
    }

    char cost_text[24] = "";
    char requests_text[24] = "";
    char tokens_text[24] = "";

    if (have_today) {
        snprintf(cost_text, sizeof cost_text, "%.2f", cost);
        snprintf(requests_text, sizeof requests_text, "%lld", requests);
        snprintf(tokens_text, sizeof tokens_text, "%llu", tokens);
    }

    char total_cost_text[24] = "";
    if (have_summary) {
        snprintf(total_cost_text, sizeof total_cost_text, "%.2f", total_cost);
    }

    const int made = snprintf(s_answer, DS_ANSWER_MAX,
                              "ok=1\n"
                              "available=%s\n"
                              "currency=%s\n"
                              "total=%.2f\n"
                              "granted=%.2f\n"
                              "topped=%.2f\n"
                              "total_cost=%s\n"
                              "today_cost=%s\n"
                              "today_requests=%s\n"
                              "today_tokens=%s\n"
                              "user_token=%s\n"
                              "hours=%s\n",
                              available ? "1" : "0", currency, total, granted, topped,
                              total_cost_text, cost_text, requests_text, tokens_text, token_state,
                              hours_text);

    if (made < 0 || (size_t)made >= DS_ANSWER_MAX) {
        set_fault(HAL_DEEPSEEK_FAULT_STORAGE, "the answer did not fit");
        return;
    }

    lock();
    s_snap.state = HAL_DEEPSEEK_DONE;
    s_snap.fault = HAL_DEEPSEEK_FAULT_NONE;
    unlock();
}

/** 一次请求，从头到尾。 */
static void read_all(void)
{
    s_have_hours = false;
    memset(s_hours, 0, sizeof s_hours);

    lock();
    const bool have_key = s_account.used && s_account.key[0] != '\0';
    const bool have_token = s_account.token[0] != '\0';
    unlock();

    if (!have_key) {
        set_fault(HAL_DEEPSEEK_FAULT_NO_CREDS, "");
        return;
    }

    double total = 0.0;
    double granted = 0.0;
    double topped = 0.0;
    char currency[8] = "CNY";
    bool available = false;

    if (!read_balance(&total, &granted, &topped, currency, sizeof currency, &available)) {
        return;
    }

    double total_cost = 0.0;
    bool have_summary = false;
    bool have_today = false;
    const char *token_state = "none";

    long long requests = 0;
    unsigned long long tokens = 0;
    double cost = 0.0;

    if (have_token) {
        const int32_t summary_fault = read_summary(&total_cost);

        if (summary_fault == HAL_DEEPSEEK_FAULT_NONE) {
            have_summary = true;
            token_state = "ok";
        } else if (summary_fault == HAL_DEEPSEEK_FAULT_AUTH) {
            /* 一枚不作数的 userToken：余额照画，用量画不成，而这句话要能到网页上 —— 它不是
             * 「没有令牌」，是「这枚过期了，再去拿一枚」。 */
            token_state = "bad";
        } else {
            token_state = "fail";
        }

        have_today = read_today(&requests, &tokens, &cost) == HAL_DEEPSEEK_FAULT_NONE;
    }

    render(available, currency, total, granted, topped, have_summary, total_cost, have_today,
           requests, tokens, cost, token_state);
}

static void ds_task(void *arg)
{
    (void)arg;

    for (;;) {
        if (xSemaphoreTake(s_wake, portMAX_DELAY) != pdTRUE) {
            continue;
        }

        read_all();
    }
}

/** 把 worker 建起来（只建一次）。 */
static esp_err_t ensure_worker(void)
{
    if (s_worker != NULL) {
        return ESP_OK;
    }

    s_wake = xSemaphoreCreateBinary();

    if (s_wake == NULL) {
        return ESP_ERR_NO_MEM;
    }

    s_stack = (StackType_t *)heap_caps_malloc(DS_TASK_STACK, MALLOC_CAP_SPIRAM);

    if (s_stack == NULL) {
        return ESP_ERR_NO_MEM;
    }

    s_task_buffer = (StaticTask_t *)heap_caps_malloc(sizeof(StaticTask_t), MALLOC_CAP_INTERNAL);

    if (s_task_buffer == NULL) {
        return ESP_ERR_NO_MEM;
    }

    s_worker = xTaskCreateStatic(ds_task, "deepseek", DS_TASK_STACK / sizeof(StackType_t), NULL, 4,
                                 s_stack, s_task_buffer);

    return (s_worker != NULL) ? ESP_OK : ESP_ERR_NO_MEM;
}

// =============================================================================
// 凭据
// =============================================================================

static bool credentials_ok(const char *value, size_t max)
{
    if (value == NULL || value[0] == '\0' || strlen(value) >= max) {
        return false;
    }

    for (const char *at = value; *at != '\0'; at++) {
        if (*at < 0x21 || *at > 0x7E || *at == '"' || *at == '\\') {
            return false;
        }
    }

    return true;
}

static esp_err_t store(void)
{
    char text[DS_KEY_MAX + DS_TOKEN_MAX + 64];
    const int made = snprintf(text, sizeof text, "key = \"%s\"\nuser = \"%s\"\n", s_account.key,
                              s_account.token);

    if (made < 0 || (size_t)made >= sizeof text) {
        return ESP_ERR_INVALID_SIZE;
    }

    /* 目录不是凭空有的：卡上第一次写凭据时它还不存在，`fopen` 直接说不行（就是那个 500）。 */
    mkdir(DS_CONF_DIR, 0777);

    FILE *file = fopen(DS_CONF_PATH, "w");

    if (file == NULL) {
        ESP_LOGE(TAG, "cannot write %s", DS_CONF_PATH);
        return ESP_FAIL;
    }

    fwrite(text, 1, (size_t)made, file);
    fclose(file);

    return ESP_OK;
}

static void load(void)
{
    FILE *file = fopen(DS_CONF_PATH, "r");

    if (file == NULL) {
        return;
    }

    char line[DS_TOKEN_MAX + 64];

    while (fgets(line, sizeof line, file) != NULL) {
        char *equals = strchr(line, '=');

        if (equals == NULL) {
            continue;
        }

        *equals = '\0';

        char *name = line;
        while (*name == ' ') {
            name++;
        }

        /* `key = "..."` 里等号前面有一个空格，切在等号上就留下了它：`strcmp("key ", "key")` 永远
         * 不成立，于是文件写得进去、开机读不回来 —— 就是「授权完重启就没了」那个 bug 的第二次。 */
        char *name_end = name + strlen(name);
        while (name_end > name && (name_end[-1] == ' ' || name_end[-1] == '\t')) {
            *--name_end = '\0';
        }

        char *value = equals + 1;
        while (*value == ' ' || *value == '"') {
            value++;
        }

        char *end = value + strlen(value);
        while (end > value && (end[-1] == '\n' || end[-1] == '\r' || end[-1] == '"' || end[-1] == ' ')) {
            end--;
        }
        *end = '\0';

        if (strcmp(name, "key") == 0) {
            snprintf(s_account.key, sizeof s_account.key, "%s", value);
            s_account.used = true;
        } else if (strcmp(name, "user") == 0) {
            snprintf(s_account.token, sizeof s_account.token, "%s", value);
        }
    }

    fclose(file);
}

esp_err_t hal_deepseek_init(void)
{
    if (s_inited) {
        return ESP_OK;
    }

    s_lock = xSemaphoreCreateMutex();

    if (s_lock == NULL) {
        return ESP_ERR_NO_MEM;
    }

    s_response = (char *)heap_caps_malloc(DS_RESPONSE_MAX, MALLOC_CAP_SPIRAM);

    if (s_response == NULL) {
        s_response = (char *)malloc(DS_RESPONSE_MAX);
    }

    if (s_response == NULL) {
        return ESP_ERR_NO_MEM;
    }

    s_response[0] = '\0';
    s_answer[0] = '\0';

    load();

    s_inited = true;

    return ESP_OK;
}

esp_err_t hal_deepseek_credentials_set(const char *key, const char *user_token)
{
    const bool have_key = key != NULL && key[0] != '\0';
    const bool have_token = user_token != NULL && user_token[0] != '\0';

    /* 两枚各自独立：网页上改一枚、另一格留空，是「只换这一枚」的意思。两格都空才是错。 */
    if (!have_key && !have_token) {
        return ESP_ERR_INVALID_ARG;
    }

    if (have_key && !credentials_ok(key, DS_KEY_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (have_token && !credentials_ok(user_token, DS_TOKEN_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_deepseek_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    if (hal_deepseek_state() == HAL_DEEPSEEK_RUNNING) {
        return ESP_ERR_INVALID_STATE;
    }

    lock();

    if (have_key) {
        snprintf(s_account.key, sizeof s_account.key, "%s", key);
    }

    if (have_token) {
        snprintf(s_account.token, sizeof s_account.token, "%s", user_token);
    }

    s_account.used = s_account.key[0] != '\0';
    unlock();

    return store();
}

esp_err_t hal_deepseek_forget(int32_t which)
{
    if (hal_deepseek_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();

    if (which == HAL_DEEPSEEK_CRED_ALL || which == HAL_DEEPSEEK_CRED_KEY) {
        s_account.key[0] = '\0';
    }

    if (which == HAL_DEEPSEEK_CRED_ALL || which == HAL_DEEPSEEK_CRED_USER) {
        s_account.token[0] = '\0';
    }

    s_account.used = s_account.key[0] != '\0';
    const bool empty = !s_account.used && s_account.token[0] == '\0';
    unlock();

    /* 一枚都不剩就把文件删了：留着空壳只会让「有没有存」多一种状态要判断。 */
    if (empty) {
        remove(DS_CONF_PATH);
        return ESP_OK;
    }

    return store();
}

esp_err_t hal_deepseek_credentials_forget(void)
{
    return hal_deepseek_forget(HAL_DEEPSEEK_CRED_ALL);
}

void hal_deepseek_user_mask(char *masked, size_t capacity)
{
    if (masked == NULL || capacity == 0) {
        return;
    }

    masked[0] = '\0';

    if (hal_deepseek_init() != ESP_OK) {
        return;
    }

    lock();
    const size_t length = strlen(s_account.token);

    if (length > 0) {
        snprintf(masked, capacity, "…%s", s_account.token + (length > 4 ? length - 4 : 0));
    }
    unlock();
}

void hal_deepseek_credentials(char *masked, size_t capacity, bool *has_user_token)
{
    if (hal_deepseek_init() != ESP_OK) {
        if (masked != NULL && capacity > 0) {
            masked[0] = '\0';
        }
        if (has_user_token != NULL) {
            *has_user_token = false;
        }
        return;
    }

    lock();
    const char *key = s_account.key;
    const size_t length = strlen(key);

    if (masked != NULL && capacity > 0) {
        if (length == 0) {
            masked[0] = '\0';
        } else if (length <= 8) {
            snprintf(masked, capacity, "…%s", key + (length > 4 ? length - 4 : 0));
        } else {
            snprintf(masked, capacity, "%.3s…%s", key, key + length - 4);
        }
    }

    if (has_user_token != NULL) {
        *has_user_token = s_account.token[0] != '\0';
    }
    unlock();
}

// =============================================================================
// 请求
// =============================================================================

esp_err_t hal_deepseek_start(void)
{
    if (hal_deepseek_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    const bool running = s_snap.state == HAL_DEEPSEEK_RUNNING;

    if (!running) {
        s_snap.state = HAL_DEEPSEEK_RUNNING;
        s_snap.fault = HAL_DEEPSEEK_FAULT_NONE;
    }
    unlock();

    if (running) {
        return ESP_ERR_INVALID_STATE;
    }

    if (ensure_worker() != ESP_OK) {
        set_fault(HAL_DEEPSEEK_FAULT_STORAGE, "no task");
        return ESP_ERR_NO_MEM;
    }

    xSemaphoreGive(s_wake);

    return ESP_OK;
}

int32_t hal_deepseek_state(void)
{
    lock();
    const int32_t state = s_snap.state;
    unlock();

    return state;
}

int32_t hal_deepseek_fault(void)
{
    lock();
    const int32_t fault = s_snap.fault;
    unlock();

    return fault;
}

esp_err_t hal_deepseek_take_answer(char *out, size_t capacity)
{
    if (out == NULL || capacity == 0) {
        return ESP_ERR_INVALID_ARG;
    }

    lock();

    if (s_snap.state != HAL_DEEPSEEK_DONE && s_snap.state != HAL_DEEPSEEK_FAILED) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    snprintf(out, capacity, "%s", s_answer);
    s_snap.state = HAL_DEEPSEEK_IDLE;
    unlock();

    return ESP_OK;
}

esp_err_t hal_deepseek_cancel(void)
{
    lock();
    if (s_snap.state == HAL_DEEPSEEK_RUNNING) {
        s_snap.state = HAL_DEEPSEEK_IDLE;
    }
    unlock();

    return ESP_OK;
}

void hal_deepseek_tick(void)
{
    /* 真板子上由网络事件驱动，这里没有事可做。 */
}

const char *hal_deepseek_fault_code(int32_t fault)
{
    switch (fault) {
    case HAL_DEEPSEEK_FAULT_NONE: return "deepseek_ok";
    case HAL_DEEPSEEK_FAULT_FAILED: return "deepseek_failed";
    case HAL_DEEPSEEK_FAULT_ARGS: return "deepseek_args";
    case HAL_DEEPSEEK_FAULT_NO_CREDS: return "deepseek_creds";
    case HAL_DEEPSEEK_FAULT_NO_NETWORK: return "deepseek_network";
    case HAL_DEEPSEEK_FAULT_UNREACHABLE: return "deepseek_unreachable";
    case HAL_DEEPSEEK_FAULT_TLS: return "deepseek_tls";
    case HAL_DEEPSEEK_FAULT_HTTP: return "deepseek_http";
    case HAL_DEEPSEEK_FAULT_AUTH: return "deepseek_auth";
    case HAL_DEEPSEEK_FAULT_API: return "deepseek_api";
    case HAL_DEEPSEEK_FAULT_STORAGE: return "deepseek_storage";
    default: return "deepseek_failed";
    }
}
