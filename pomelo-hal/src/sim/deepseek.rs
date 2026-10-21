//! A DeepSeek account that exists only on this desktop.
//!
//! 和 [`SimWorkBuddy`][crate::sim::SimWorkBuddy] 同样的形状：没有网络，答案在 `tick` 里到，
//! 用它把页面画出来对一遍。

use crate::error::HalError;
use crate::types::{
    DeepSeekAccount, DeepSeekAnswer, DeepSeekFault, DeepSeekReport, DeepSeekState, UserTokenState,
};

/// 一台模拟的 DeepSeek 账号。
#[derive(Debug, Clone, Default)]
pub struct SimDeepSeek {
    account: DeepSeekAccount,
    state: DeepSeekState,
    answer: Option<DeepSeekAnswer>,
    ticks: u32,
}

impl SimDeepSeek {
    /// 一个存好了 Key 的模拟账号。
    pub fn new() -> Self {
        Self {
            account: DeepSeekAccount {
                masked_key: String::from("sk-…4ab8"),
                has_user_token: true,
            },
            ..Self::default()
        }
    }

    /// 一个什么都没存的模拟账号。
    pub fn empty() -> Self {
        Self::default()
    }
}

impl crate::traits::DeepSeekBackend for SimDeepSeek {
    fn account(&self) -> DeepSeekAccount {
        self.account.clone()
    }

    fn remember(&mut self, key: &str, user_token: Option<&str>) -> Result<(), HalError> {
        if key.trim().is_empty() {
            return Err(HalError::InvalidArg);
        }

        self.account.masked_key = format!("{}…{}", &key[..key.len().min(3)], &key[key.len().saturating_sub(4)..]);
        self.account.has_user_token = user_token.is_some_and(|token| !token.trim().is_empty());

        Ok(())
    }

    fn forget(&mut self) -> Result<(), HalError> {
        self.account = DeepSeekAccount::default();
        Ok(())
    }

    fn start(&mut self) -> Result<(), HalError> {
        if matches!(self.state, DeepSeekState::Running) {
            return Err(HalError::Busy);
        }

        if !self.account.configured() {
            self.state = DeepSeekState::Failed(DeepSeekFault::NoKey);
            self.answer = Some(DeepSeekAnswer::Refused {
                fault: DeepSeekFault::NoKey,
                message: String::new(),
            });
            return Ok(());
        }

        self.state = DeepSeekState::Running;
        self.ticks = 0;

        Ok(())
    }

    fn state(&self) -> DeepSeekState {
        self.state
    }

    fn take_answer(&mut self) -> Result<DeepSeekAnswer, HalError> {
        let answer = self.answer.take().ok_or(HalError::Busy)?;
        self.state = DeepSeekState::Idle;

        Ok(answer)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        self.state = DeepSeekState::Idle;
        self.answer = None;

        Ok(())
    }

    fn tick(&mut self) {
        if !matches!(self.state, DeepSeekState::Running) {
            return;
        }

        self.ticks += 1;

        if self.ticks < 2 {
            return;
        }

        self.answer = Some(DeepSeekAnswer::Report(DeepSeekReport {
            available: true,
            currency: String::from("CNY"),
            total: Some(109.02),
            granted: Some(33.97),
            topped: Some(75.05),
            total_cost: Some(712.92),
            today_cost: Some(0.38),
            today_requests: Some(12),
            today_tokens: Some(746_227_521),
            user_token: UserTokenState::Ok,
            hours: vec![0, 0, 1, 0, 2, 8, 14, 3, 0, 5, 9, 12, 6, 4, 10, 7, 2, 0, 1, 3, 0, 0, 0, 0],
        }));
        self.state = DeepSeekState::Done;
    }
}
