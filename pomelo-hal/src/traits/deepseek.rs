//! The board as a reader of one DeepSeek account's money.
//!
//! The fifth of the network traits, and the narrowest: two documents, both belonging to somebody
//! else's billing, and neither of them anything this board can change. It reads; it never writes.
//!
//! # 两份文档，两种凭据
//!
//! - 余额走官方公开接口 `api.deepseek.com/user/balance`，凭的是一枚 **API Key**。稳，但它只说
//!   余额：总额、充值、赠送，别的什么都没有。
//! - 用量走开放平台的网页接口（`platform.deepseek.com/api/v0/...`），凭的是一枚控制台
//!   **userToken**。它给当天的请求次数、token 数、消费金额 —— 也正因如此它是私有的，可能变。
//!
//! 所以这里有两个可选凭据而不是一个：只给 Key 就是三个数，再给 userToken 才是一整页。
//!
//! # 先发起，再取
//!
//! [`DeepSeekBackend::start`] 立刻返回；结果看 [`DeepSeekBackend::state`]，答案用
//! [`DeepSeekBackend::take_answer`] 取。一次请求是一个 TLS 握手加一次读，等它的页面就是不画的页面。

use crate::error::HalError;
use crate::types::{DeepSeekAccount, DeepSeekAnswer, DeepSeekState};

/// Reading one DeepSeek account's balance, and what it spent today.
pub trait DeepSeekBackend: Send + Sync {
    /// The credentials on file, as much of them as a page gets to see.
    ///
    /// Cheap, and safe every frame. Nothing saved is a screen ("paste one on the management page"),
    /// not a failure.
    fn account(&self) -> DeepSeekAccount;

    /// Saves the API key and, optionally, the console's `userToken`.
    ///
    /// [`HalError::InvalidArg`] for a key that is not one, and [`HalError::Busy`] while a request is
    /// in flight — the worker reads these in place.
    fn remember(&mut self, key: &str, user_token: Option<&str>) -> Result<(), HalError>;

    /// Forgets both. Nothing saved is `Ok(())`.
    fn forget(&mut self) -> Result<(), HalError>;

    /// Asks for the balance (and today's usage, when there is a `userToken`), and returns at once.
    fn start(&mut self) -> Result<(), HalError>;

    /// How the request is going. Cheap, and safe every frame.
    fn state(&self) -> DeepSeekState;

    /// Takes the answer of a finished request and clears it.
    fn take_answer(&mut self) -> Result<DeepSeekAnswer, HalError>;

    /// Abandons the request in flight. Nothing to abandon is `Ok(())`.
    fn cancel(&mut self) -> Result<(), HalError>;

    /// Advances a simulated request. Native, event-driven backends can ignore this.
    fn tick(&mut self) {}
}

/// 一个什么都不做的后端。
///
/// [`Board`][crate::Board] 的每个字段都是一个 `Box<dyn …>`，没有 `Option` 那一层 —— 一种「这块
/// 板子没有这个东西」的状态，最省事的说法就是一个什么都答不上来的实现。固件在装配的时候把真的
/// 换进去（[`Board::set_deepseek`][crate::Board::set_deepseek]）。
#[derive(Debug, Default)]
pub struct NoDeepSeek;

impl DeepSeekBackend for NoDeepSeek {
    fn account(&self) -> DeepSeekAccount {
        DeepSeekAccount::default()
    }

    fn remember(&mut self, _key: &str, _user_token: Option<&str>) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    fn forget(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), HalError> {
        Err(HalError::NotSupported)
    }

    fn state(&self) -> DeepSeekState {
        DeepSeekState::Idle
    }

    fn take_answer(&mut self) -> Result<DeepSeekAnswer, HalError> {
        Err(HalError::Busy)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        Ok(())
    }
}
