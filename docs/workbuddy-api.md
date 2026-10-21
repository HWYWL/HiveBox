# WorkBuddy 接口与接入流程（板子这一侧）

这块板子在这套系统里是一个**外部客户端**：用设备授权拿一枚 JWT，之后用 Bearer 调四个
billing 接口，把结果画在面板上、存在 `/internal` 里。

本文只写我们真的在用的东西 —— 每条路径、每个字段名、每个坑都对应
`pomelo-firmware/firmware/components/board_hal/board_workbuddy.c` 里的一行代码。没验过的接口不写。

---

## 1. 主机、身份、公共请求头

| 项 | 值 |
|---|---|
| 主机 | `https://www.workbuddy.cn`（默认；国际版是 `www.workbuddy.ai`，逻辑相同） |
| 端口 | 443，走 TLS，证书用 `esp_crt_bundle` 校验 |
| 超时 | 15 s |
| 重试 | 2 次，且只在「网络类」故障时重试（连不上 / TLS 握手失败），4xx 不重试 |

每次业务调用都带这几个头：

```
Authorization: Bearer <accessToken>
X-Domain:      www.workbuddy.cn        # 与请求的主机一致
Accept:        application/json
User-Agent:    WorkBuddy/5.4.2
X-Product:     SaaS
```

登录那两个接口**不带** `Authorization`，而是带一组显式的「没有」：

```
X-No-Authorization:   true
X-No-User-Id:         true
X-No-Enterprise-Id:   true
X-No-Department-Info: true
```

> 坑：这套接口的分组不统一。签到走 `/v2/billing/meter/*`，余额和用量走 `/billing/meter/*`
> （**没有** `/v2`）。少一个前缀就是 404，多一个也是。

---

## 2. 登录：设备授权（不用手抄令牌）

板子不会让你在 480 像素的屏上敲两 KB 的 JWT，所以走 App 自己的设备授权流程，从外面看是两步：

```
POST /v2/plugin/auth/state?platform=workbuddy-ai
  -> {"code":0,"data":{"state":"<uuid>","authUrl":"https://www.workbuddy.cn/login?platform=workbuddy-ai&state=<uuid>"}}

   （人打开 authUrl —— 手机扫它画出来的码 —— 在浏览器里授权）

GET  /v2/plugin/auth/token?state=<uuid>
  -> {"code":11217}                                   # 还在等人
  -> {"code":0,"data":{"accessToken":"eyJ...","refreshToken":"eyJ..."}}
```

要点：

- `state` 把两次调用绑在一起，有效期以服务端为准（我们不管，拿不到就一直问）。
- `11217` 是「还在等」，不是错误。板子把它当「继续轮询」，一秒问一次。
- `authUrl` 本身就是二维码的内容：网页上给的是链接，将来面板上要画码也是画它。
- 拿到的 `accessToken` / `refreshToken` 都是 JWT，`exp` 里是过期时间戳，板子自己解出来算天数。

---

## 3. 四个业务接口

全部是 `POST`，请求体除了最后一个是 `{}`。

### 3.1 签到状态

```
POST /v2/billing/meter/checkin-activity-status
body: {}
```

响应（`data` 之下）：

| 字段 | 含义 | 我们怎么用 |
|---|---|---|
| `today_checked_in` | 今天签没签 | 「今天已签 / 今天还没签」 |
| `streak_days` | 连签天数 | `连续 5 天` |
| `daily_credit` | 每天给多少分 | `每天 100 积分` |
| `total_credits` | 活动累计给了多少分 | `累计 500` |
| `theme_name` | 活动名 | 详情行末尾（会过滤掉字体画不出的字） |

### 3.2 执行签到

```
POST /v2/billing/meter/daily-checkin
body: {}
```

返回码决定说什么：

| `code` | 含义 | 处理 |
|---|---|---|
| `0` | 签到成功 | 读出 `today_credit`（或 `credit`）说「+N 积分」 |
| `10001` | 今天已经签过 | 不是失败，说「今天已经签过了」，然后照常刷新状态 |

### 3.3 余额（剩余 / 总额）

```
POST /billing/meter/get-user-resource-summary
body: {}
```

响应（`data`）：

```json
{"Packages":[
   {"PackageCode":"TCACA_code_007_...","CycleTotalCapacity":"8400","CycleRemainCapacity":"2200",
    "CycleUsedCapacity":"6200","CapacityUnit":"credits","TotalCount":27}
 ],
 "TotalCredits":"11069.37000014",
 "CreditGroups":[ ... 同样的数额，按种类拆开 ... ]}
```

- **剩余**取 `TotalCredits`（接口自己算好的，别自己加）。
- **总额**是 `Packages[]` 里 `CycleTotalCapacity` 之和。
- 两个数值都是**带引号的字符串**，`strtod` 前要先跳过引号 —— 直接 `strtod("\"2200\"")` 会得 0。
- 数组外面还有一份 `CreditGroups`，字段名与 `Packages` 相同。求和时必须停在 `]` 上，
  否则每个数翻倍（我们踩过）。

### 3.4 用量（本月）

```
POST /billing/meter/get-user-resource
body: {}
```

外层比别的接口多包了两层：`data.Response.Data`。

| 字段 | 含义 |
|---|---|
| `TotalCount` | 有几种资源 |
| `TotalDosage` | 本月已用次数（**服务端自己滞后两三小时**） |
| `Accounts[]` | 每个资源一份，带 `PackageName`、`CycleEndTime`、`CycleCapacityRemain` |

我们用 `Accounts[]` 找「最近哪个包到期、到期多少分」：按 `CycleEndTime`（`"2026-11-21 14:50:00"`）
取最小，只留还有余额（`CycleCapacityRemain > 0`）的那些。

这份文档随账号使用历史增长，一个用得多的账号四十多 KB —— 缓冲区给到 96 KB（PSRAM）。

### 3.5 逐请求流水（今天）

```
POST /billing/meter/get-user-request-usage
body: {"startTime":"2026-10-21 00:00:00","endTime":"2026-10-21 23:59:59","pageNum":1,"pageSize":50}
```

响应：

```json
{"data":{"total":42,"data":[
  {"requestId":"aa82...","credit":7.39999999,"model":"deepseek-v4.1-flash",
   "client":"CodeBuddyIDE","requestTime":"2026-10-21 15:20:00",
   "inputTrunc":"<提示词前几百字>","input":"<提示词全文>","appType":"codebuddy"}
]}}
```

- `total` 是当天的请求条数；`data[]` 是这一页的行。
- **没有 token 数**。整套账号接口里都没有 token —— 只有 `credit`（积分）。参考项目里的
  「Token 统计」是扫本机会话日志算出来的，板子拿不到。
- 每行都带**整段提示词**（`input` 全文、`inputTrunc` 前几百字）。这是别人写的东西，
  板子只取 `credit` / `model` / `requestTime`，其余一律不落盘、不出页面。
- 行很大（约 1 KB/行），所以：每页 50 条、一天最多读 4 页（200 条），整份结果缓存 5 分钟。
  极忙的一天这两个数是下限，不是精确值。

---

## 4. 板子对外的 HTTP 接口

面板和网页都在这台板子上，`board_web.c` 暴露这些（无鉴权，仅局域网）：

| 方法 | 路径 | 作用 |
|---|---|---|
| `GET` | `/api/workbuddy/status` | 存没存令牌、状态机在哪、故障码 |
| `POST` | `/api/workbuddy/token` | 表单 `token=&refresh=&host=` 直接塞一枚令牌（手工兜底） |
| `POST` | `/api/workbuddy/forget` | 删掉令牌与文件 |
| `POST` | `/api/workbuddy/login?action=start` | 申请 `state`，返回 `{"state":…,"url":…}` |
| `POST` | `/api/workbuddy/login?action=poll` | 问一次授权结果；成功后令牌自动落盘 |
| `POST` | `/api/workbuddy/start?action=status\|checkin` | 发起一次读取 / 一次签到 |
| `GET` | `/api/workbuddy/answer` | 取回上一条答案原文（`ok=1\ntoday_credit=…` 那种） |
| `GET` | `/api/workbuddy/raw?which=status\|credits\|usage\|requests` | **调试用**：接口原话，逐请求那条带 `&day=&size=&page=` |

`/api/workbuddy/start` 与 `login` 都是一次一答：HTTP 处理线程等 worker 的结果，
所以它们会**阻塞到有答案**（`start` 最多等 20 s，`login` / `raw` 最多 25 s）。
同时只能跑一件，撞上会回 409。

---

## 5. 落盘

`/internal/AppData/WORKBUDDY/workbuddy.conf`，三行 TOML 风格：

```
host = "www.workbuddy.cn"
token = "eyJhbGciOi..."
refresh = "eyJhbGciOi..."
```

> 坑：写的是 `key = "…"`，读的扫描器必须认这种形状。曾经读的那段只认 JSON 的
> `"key":"…"`，于是每次开机都读不回来 —— 表现就是「授权完重启就没了」。

---

## 6. 数据口径与坑（都是踩过的）

1. **`credit` 不是 token。** 账号接口按积分记账，一次请求几个 credit 与输入输出多少 token
   没有线性关系。面板上说「分」，不说「token」。
2. **`TotalDosage` 滞后。** 服务端自己慢两三小时，所以它只能当「本月」，不能冒充「今天」。
   今天的数是 3.5 里逐条数出来的。
3. **带引号的数字。** 余额文档里容量是字符串，直接 `strtod` 得 0（会把真账号画成「0.0 / 0.0」）。
4. **数组要有边界。** 同一个文档里有第二份同样的数额（`CreditGroups`），扫过 `]` 就翻倍。
5. **401 不等于连不上。** `esp_http_client` 对带 `WWW-Authenticate: Bearer` 的 401 返回
   `ESP_ERR_NOT_SUPPORTED`，但状态码是可读的 —— 按状态码分类，否则过期令牌会显示「网络故障」。
6. **截断不等于空。** 文档读到一半（缓冲区不够）时，数组没有收尾的 `]`；这时按缓冲区末尾继续读，
   不要整段丢弃。
7. **画不出的字就是方框。** 活动名里有 emoji，字体只有拉丁与 CJK；过滤掉画不出的字，
   比画一串方框好。
8. **没答上来画「—」，不画 0。** 「没取到」和「用光了」是两句话。

---

## 7. 故障码（`HAL_WORKBUDDY_FAULT_*`）

| 码 | 常量 | 什么时候 |
|---|---|---|
| 0 | `NONE` | 正常 |
| 1 | `ARGS` | 不认识的动作 |
| 2 | `NO_CREDS` | 还没令牌 |
| 3 | `NO_NETWORK` | 板子没联网 |
| 4 | `UNREACHABLE` | DNS / 连接 / 读失败 |
| 5 | `TLS` | 握手失败（证书或时间） |
| 6 | `HTTP` | 非 2xx，且不是 401/403 |
| 7 | `AUTH` | 401/403：令牌过期或不对 |
| 8 | `API` | 账号拒绝（`message` 带原因） |
| 9 | `STORAGE` | 答案放不下 / 令牌文件读写失败 |

---

## 8. 一条答案长什么样

`/api/workbuddy/answer` 与网页「读取状态」拿到的都是这个纯文本（一次实测，已脱敏）：

```
ok=1
checked_in=1
streak=5
daily=100
activity_total=500
activity=Buddy
access_days=27
refresh_days=29
credit_remaining=11026.9
credit_total=17400.0
usage_kinds=29
usage_calls=110526
packages=20
expire=11-21
expire_credit=100.0
today_requests=42
today_credit=173.6
today_model=deepseek-v4.1-flash
```

`pomelo-hal/src/workbuddy_answer.rs` 把这个文本解析成 `WorkBuddyReport`，应用只读结构体，
不碰 JSON，也不碰网络。
