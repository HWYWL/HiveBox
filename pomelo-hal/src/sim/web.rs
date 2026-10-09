//! Simulated management server.

use crate::error::HalError;
use crate::traits::WebBackend;
use crate::types::WebStatus;

/// A management server that is up when it is asked to be, and serves nothing.
///
/// It binds no socket and has no page to serve — the page is the firmware's, and a desktop is not
/// what a browser on the same network would reach. What it keeps is the part the *app* can see, so
/// the switch, the address line and the tests around them are one implementation on both sides of
/// the `target_os` line.
#[derive(Debug, Default)]
pub struct SimWeb {
    status: WebStatus,
}

impl SimWeb {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WebBackend for SimWeb {
    fn start(&mut self, port: u16) -> Result<(), HalError> {
        // The same refusal the device makes: a port of zero is a request to be reachable at nothing.
        if port == 0 {
            return Err(HalError::InvalidArg);
        }

        self.status = WebStatus {
            running: true,
            port,
        };

        Ok(())
    }

    fn stop(&mut self) -> Result<(), HalError> {
        self.status = WebStatus::default();

        Ok(())
    }

    fn status(&self) -> WebStatus {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starting twice, stopping twice, and the port that comes back.
    #[test]
    fn the_switch_remembers_where_it_was_put() {
        let mut web = SimWeb::new();

        assert_eq!(web.status(), WebStatus::default());

        web.start(80).expect("a first start");

        assert_eq!(
            web.status(),
            WebStatus {
                running: true,
                port: 80
            }
        );

        // Asking again on the same port is not an error and not a restart.
        web.start(80).expect("a second start");

        assert_eq!(web.status().port, 80);

        web.stop().expect("a stop");

        // Down has no port: an URL built from a stopped server would be an address that answers
        // nothing, and the status is where that is decided.
        assert_eq!(web.status(), WebStatus::default());
        assert_eq!(web.status().port, 0);

        // Stopping what is already stopped is what a page that was closed twice does.
        web.stop().expect("a second stop");
    }

    #[test]
    fn a_port_of_zero_is_refused() {
        let mut web = SimWeb::new();

        assert_eq!(web.start(0), Err(HalError::InvalidArg));
        assert!(!web.status().running);
    }
}
