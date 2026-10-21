//! 一条 DeepSeek 答案的解析：从一段 `key=value` 的纯文本到 [`DeepSeekAnswer`]。
//!
//! C 侧交回来的是一段文本，不是 JSON —— 和 `workbuddy_answer` 一个道理：JSON 在板子上只能靠
//! 一个几百行的手写扫描器去读，而这里要的东西全都是数字，一行一个，读错了也看得出来。
//!
//! # 形状
//!
//! ```text
//! ok=1
//! available=1
//! currency=CNY
//! total=109.02
//! granted=33.97
//! topped=75.05
//! total_cost=712.92
//! today_cost=0.38
//! today_requests=12
//! today_tokens=123456
//! user_token=ok | bad | fail | none
//! hours=0,0,12,3,…（24 个，今天每个小时的请求数）
//! ```
//!
//! 空值就是不知道：`today_cost=` 表示没有 userToken，画破折号。
//!
//! 失败时是两行：
//!
//! ```text
//! error=deepseek_auth
//! message=Authorization Failed (invalid token)
//! ```

use crate::types::{DeepSeekAnswer, DeepSeekFault, DeepSeekReport, UserTokenState};

/// 把一段答案读成一样东西。
pub fn parse(text: &str) -> DeepSeekAnswer {
    let mut report = DeepSeekReport::default();
    let mut fault: Option<DeepSeekFault> = None;
    let mut message = String::new();

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let value = value.trim();

        match key.trim() {
            "error" => fault = Some(fault_of(value)),
            "message" => message = value.to_string(),
            "available" => report.available = value == "1" || value.eq_ignore_ascii_case("true"),
            "currency" => report.currency = value.to_string(),
            "total" => report.total = decimal(value),
            "granted" => report.granted = decimal(value),
            "topped" => report.topped = decimal(value),
            "total_cost" => report.total_cost = decimal(value),
            "today_cost" => report.today_cost = decimal(value),
            "today_requests" => report.today_requests = number(value),
            "today_tokens" => report.today_tokens = whole(value),
            "hours" => {
                report.hours = value
                    .split(',')
                    .filter_map(|hour| hour.trim().parse::<u32>().ok())
                    .collect();
            }
            "user_token" => {
                report.user_token = match value {
                    "ok" => UserTokenState::Ok,
                    "bad" => UserTokenState::Bad,
                    "fail" => UserTokenState::Fail,
                    _ => UserTokenState::None,
                };
            }
            _ => {}
        }
    }

    match fault {
        Some(fault) => DeepSeekAnswer::Refused { fault, message },
        None => DeepSeekAnswer::Report(report),
    }
}

/// 一个故障码，翻回枚举里那一条。
fn fault_of(code: &str) -> DeepSeekFault {
    match code {
        "deepseek_args" => DeepSeekFault::Failed,
        "deepseek_creds" => DeepSeekFault::NoKey,
        "deepseek_network" => DeepSeekFault::NoNetwork,
        "deepseek_unreachable" => DeepSeekFault::Unreachable,
        "deepseek_tls" => DeepSeekFault::Tls,
        "deepseek_http" => DeepSeekFault::Http,
        "deepseek_auth" => DeepSeekFault::Auth,
        "deepseek_api" => DeepSeekFault::Api,
        "deepseek_storage" => DeepSeekFault::Storage,
        _ => DeepSeekFault::Failed,
    }
}

/// 一个可能有小数的数。空字符串是「不知道」，不是 0。
fn decimal(value: &str) -> Option<f32> {
    if value.is_empty() {
        return None;
    }

    value.parse::<f32>().ok()
}

/// 一个整数。
fn number(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }

    value.parse::<u32>().ok()
}

/// 一个可能很大的整数：token 数以亿计，`u32` 装得下但没必要冒险。
fn whole(value: &str) -> Option<u64> {
    if value.is_empty() {
        return None;
    }

    value.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_read_into_numbers() {
        let answer = parse(
            "ok=1\navailable=1\ncurrency=CNY\ntotal=109.02\ngranted=33.97\ntopped=75.05\n\
             total_cost=712.92\ntoday_cost=0.38\ntoday_requests=12\ntoday_tokens=746227521\n",
        );

        let DeepSeekAnswer::Report(report) = answer else {
            panic!("应该是一份账目");
        };

        assert!(report.available);
        assert_eq!(report.currency, "CNY");
        assert_eq!(report.total, Some(109.02));
        assert_eq!(report.granted, Some(33.97));
        assert_eq!(report.topped, Some(75.05));
        assert_eq!(report.total_cost, Some(712.92));
        assert_eq!(report.today_requests, Some(12));
        assert_eq!(report.today_tokens, Some(746_227_521));
    }

    #[test]
    fn a_key_without_a_user_token_has_no_usage() {
        let answer = parse("ok=1\navailable=1\ncurrency=CNY\ntotal=1.0\ntoday_cost=\n");

        let DeepSeekAnswer::Report(report) = answer else {
            panic!("应该是一份账目");
        };

        assert_eq!(report.today_cost, None, "没答上来是不知道，不是 0");
        assert_eq!(report.today_requests, None);
    }

    #[test]
    fn the_hours_come_back_as_twenty_four_numbers() {
        let answer = parse("ok=1\nuser_token=ok\nhours=0,1,2,3\n");

        let DeepSeekAnswer::Report(report) = answer else {
            panic!("应该是一份账目");
        };

        assert_eq!(report.hours, vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_refusal_carries_the_sentence() {
        let answer = parse("error=deepseek_auth\nmessage=Authorization Failed (invalid token)\n");

        let DeepSeekAnswer::Refused { fault, message } = answer else {
            panic!("应该是一句拒绝");
        };

        assert_eq!(fault, DeepSeekFault::Auth);
        assert!(message.contains("Authorization Failed"));
    }
}
