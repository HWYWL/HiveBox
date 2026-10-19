//! The simulated cloud account: two vendors, four machines, and a round trip that takes three ticks.
//!
//! What it exists for is the panel app: a page that lists somebody's servers cannot be built on a
//! desktop without servers, and the C side's answer document cannot be produced without a network.
//! So this backend invents both — the account and the *timing*, which is the half that is easy to
//! forget: a request that answered in the same frame it was asked for would let a page be written
//! that never draws a spinner, and the spinner is most of what a person sees on the device.
//!
//! # The machines are not the same machine
//!
//! One product has two instances and another has one, and the sets are deliberately not alike: an
//! instance that is up with an address, one that is down with none, one product with a name somebody
//! chose and one whose name is its id, and machines in two different data centres. A list of four
//! identical rows would hide every layout question a page has to answer.
//!
//! # What can be asked of it
//!
//! [`SimCloud::without_key`] takes a vendor's credentials away, which is the one screen a page has
//! that a happy-path simulator never reaches, and [`SimCloud::failing`] makes every request end in a
//! chosen fault. Both are builders rather than knobs to turn at runtime: what they are for is a
//! demo's `main.rs` and a test, and both of those know what they want before the first frame.
//!
//! AWS is here the way the board has it: a vendor with a row and no client. A list of it answers
//! [`CloudFault::Unsupported`] whether or not a key is saved, which is the sentence the page has to
//! be able to say and the one thing a happy-path simulator would never produce.

use crate::error::HalError;
use crate::traits::CloudBackend;
use crate::types::{
    CloudAction, CloudAnswer, CloudFault, CloudInstance, CloudInstanceState, CloudKey, CloudProvider,
    CloudState, CloudTarget, CloudVendor,
};

/// How many ticks an invented request takes before its answer is ready.
///
/// Three, and named rather than inlined because it is the whole of the simulator's *feel*: a page
/// polls once a frame, so three ticks is three frames of spinner on a desktop and three seconds on
/// the board — which is about what a signed round trip to a data centre takes. See
/// [`CloudBackend::tick`].
const LATENCY: u32 = 3;

/// The region the invented Tencent account's machines are in.
const REGION: &str = "ap-guangzhou";

/// The region the invented Alibaba account's machine is in, spelled the way Alibaba spells it.
const ALIYUN_REGION: &str = "cn-hangzhou";

/// A cloud account that is not there.
pub struct SimCloud {
    /// The key per vendor, indexed by [`CloudVendor::index`].
    keys: [CloudKey; 3],
    /// The instances per product, indexed by [`CloudProvider::index`].
    instances: [Vec<CloudInstance>; 4],
    /// The request in flight, if there is one.
    in_flight: Option<Request>,
    /// The answer waiting to be taken, if there is one.
    answer: Option<CloudAnswer>,
    /// How the last request went, which is what a page polls.
    state: CloudState,
    /// A fault every request ends in, when a demo asked for one.
    fault: Option<CloudFault>,
    /// How many requests this simulator has settled, for the receipts it hands out.
    serial: u32,
}

/// The request in flight: what was asked, and how long is left of the round trip.
struct Request {
    target: CloudTarget,
    action: CloudAction,
    instance: String,
    ticks_left: u32,
}

impl SimCloud {
    /// An account with two vendors set up and four machines on it.
    pub fn new() -> Self {
        let tencent = CloudKey {
            masked: String::from("AKID1a2b……7z9y"),
            region: String::from(REGION),
        };

        let aliyun = CloudKey {
            masked: String::from("LTAI4G……8k2m"),
            region: String::from(ALIYUN_REGION),
        };

        Self {
            keys: [tencent, aliyun, CloudKey::default()],
            instances: [
                vec![
                    CloudInstance {
                        id: String::from("ins-a1b2c3d4"),
                        name: String::from("web-01"),
                        state: CloudInstanceState::Running,
                        ip: String::from("203.0.113.7"),
                        provider: CloudProvider::Cvm,
                        region: String::from(REGION),
                    },
                    CloudInstance {
                        id: String::from("ins-e5f6a7b8"),
                        name: String::from("db-01"),
                        state: CloudInstanceState::Stopped,
                        ip: String::new(),
                        provider: CloudProvider::Cvm,
                        region: String::from(REGION),
                    },
                ],
                vec![CloudInstance {
                    id: String::from("lhins-9x8y7z"),
                    name: String::from("blog"),
                    state: CloudInstanceState::Running,
                    ip: String::from("198.51.100.42"),
                    provider: CloudProvider::Lighthouse,
                    /* Not the same data centre as the other two: a vendor's list is also a merge of
                     * regions, and a demo that put everything in one would never show a row whose
                     * power command has to name a region. */
                    region: String::from("ap-tokyo"),
                }],
                vec![CloudInstance {
                    id: String::from("i-bp1xy2z3"),
                    name: String::from("api-01"),
                    state: CloudInstanceState::Running,
                    ip: String::from("47.96.1.2"),
                    provider: CloudProvider::Ecs,
                    region: String::from(ALIYUN_REGION),
                }],
                // AWS has no client in this build, so it has no machines to invent.
                Vec::new(),
            ],
            in_flight: None,
            answer: None,
            state: CloudState::Idle,
            fault: None,
            serial: 0,
        }
    }

    /// The same account with `vendor`'s key taken away.
    ///
    /// The screen a board is on before anybody has typed a key in, and one a page has to draw
    /// rather than skip: nothing about it is a failure, and what it has to say is where the key
    /// goes in.
    pub fn without_key(mut self, vendor: CloudVendor) -> Self {
        self.keys[vendor.index()] = CloudKey::default();
        self
    }

    /// The same account on a bad day: every request ends in `fault`.
    ///
    /// A fault and not a provider refusal: what a fault draws is the page's own words for it, which
    /// is the half of a page's error handling that has no text from anywhere else to show.
    pub fn failing(mut self, fault: CloudFault) -> Self {
        self.fault = Some(fault);
        self
    }

    /// The instances of one vendor, as its list request would answer with them.
    ///
    /// A merge, in the order the page draws the products and the order the C side's answer puts them
    /// in — and only from a product whose vendor has a key, which is the same rule the board
    /// follows: a vendor nobody typed a key for contributes no machines and no complaint.
    fn list_of(&self, vendor: CloudVendor) -> Vec<CloudInstance> {
        if !self.keys[vendor.index()].configured() {
            return Vec::new();
        }

        vendor
            .products()
            .iter()
            .flat_map(|product| self.instances[product.index()].clone())
            .collect()
    }

    /// Turns the request in flight into the answer it earned.
    fn settle(&mut self, request: Request) {
        let target = request.target;

        let vendor = match target {
            CloudTarget::Vendor(vendor) => vendor,
            CloudTarget::Machine(product) => product.vendor(),
        };

        /* A vendor this build has no client for says so before anything else, even with a key saved:
         * that is the whole of what "AWS is in the list" means today. */
        let keyed = self.keys[vendor.index()].configured();

        if let Some(fault) = self.fault.or_else(|| {
            if !vendor.is_wired_up() {
                Some(CloudFault::Unsupported)
            } else if !keyed {
                Some(CloudFault::NoCredentials)
            } else {
                None
            }
        }) {
            self.state = CloudState::Failed(fault);
            // No code and no sentence: those two fields are for what a *provider* wrote down, and
            // a fault is this side's own account of what happened. See `CloudAnswer::Refused`.
            self.answer = Some(CloudAnswer::Refused {
                fault,
                code: String::new(),
                message: String::new(),
            });
            return;
        }

        self.serial += 1;

        self.state = CloudState::Done;
        self.answer = Some(match request.action {
            CloudAction::List => CloudAnswer::Instances {
                vendor,
                /* A list has no one region: it spans every data centre of every product, and each
                 * row carries its own. */
                region: String::new(),
                instances: self.list_of(vendor),
                // The simulator's products always answer: there is no network here to lose half of
                // a list to.
                failed: Vec::new(),
            },
            action => {
                let CloudTarget::Machine(product) = target else {
                    // Unreachable in practice: `start` refuses a power command that names a vendor.
                    self.state = CloudState::Failed(CloudFault::Args);
                    self.answer = Some(CloudAnswer::Refused {
                        fault: CloudFault::Args,
                        code: String::new(),
                        message: String::new(),
                    });
                    return;
                };

                // The command has landed by now: the instance was put into its in-between state
                // when the request started, and this is the other end of it. See `start`.
                if let Some(instance) = self.instances[product.index()]
                    .iter_mut()
                    .find(|instance| instance.id == request.instance)
                {
                    instance.state = match action {
                        CloudAction::Stop => CloudInstanceState::Stopped,
                        _ => CloudInstanceState::Running,
                    };
                    if action == CloudAction::Stop {
                        instance.ip = String::new();
                    }

                    CloudAnswer::Accepted {
                        provider: product,
                        action,
                        request_id: format!("sim-{:04}", self.serial),
                    }
                } else {
                    // An instance that is not in the list: the same answer the provider would give,
                    // by the same route, since a page has to handle it either way.
                    self.state = CloudState::Failed(CloudFault::Api);
                    CloudAnswer::Refused {
                        fault: CloudFault::Api,
                        code: String::from("InvalidInstanceId.NotFound"),
                        message: String::from("instance not found"),
                    }
                }
            }
        });
    }
}

impl Default for SimCloud {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudBackend for SimCloud {
    fn key(&self, vendor: CloudVendor) -> CloudKey {
        self.keys.get(vendor.index()).cloned().unwrap_or_default()
    }

    fn start(
        &mut self,
        target: CloudTarget,
        action: CloudAction,
        instance: &str,
        _region: &str,
    ) -> Result<(), HalError> {
        if matches!(self.state, CloudState::Running) {
            return Err(HalError::Busy);
        }

        if action.is_power() && instance.is_empty() {
            return Err(HalError::InvalidArg);
        }

        // A vendor is a list's target and nothing else: a power command names one machine, and a
        // machine belongs to one product's API. The board refuses it, and so does the simulator.
        if action.is_power() && !matches!(target, CloudTarget::Machine(_)) {
            return Err(HalError::InvalidArg);
        }

        // A list names a vendor, and the board refuses a product there for the same reason: the
        // number ranges do not overlap, and neither does this.
        if !action.is_power() && !matches!(target, CloudTarget::Vendor(_)) {
            return Err(HalError::InvalidArg);
        }

        // A power command shows up in the list *before* the provider answers, which is what the
        // device does too: the command is on its way and the machine is already changing. It is
        // also what makes a page's "busy" state reachable at all — a row that only went busy once
        // the answer arrived would never be seen busy.
        if action.is_power() {
            if let CloudTarget::Machine(product) = target {
                if let Some(found) = self.instances[product.index()]
                    .iter_mut()
                    .find(|found| found.id == instance)
                {
                    found.state = match action {
                        CloudAction::Start => CloudInstanceState::Starting,
                        CloudAction::Stop => CloudInstanceState::Stopping,
                        _ => CloudInstanceState::Rebooting,
                    };
                }
            }
        }

        self.answer = None;
        self.state = CloudState::Running;
        self.in_flight = Some(Request {
            target,
            action,
            instance: String::from(instance),
            ticks_left: LATENCY,
        });

        Ok(())
    }

    fn state(&self) -> CloudState {
        self.state
    }

    fn take_answer(&mut self) -> Result<CloudAnswer, HalError> {
        if matches!(self.state, CloudState::Idle | CloudState::Running) {
            return Err(HalError::Busy);
        }

        let answer = self.answer.take().ok_or(HalError::Busy)?;
        self.state = CloudState::Idle;
        Ok(answer)
    }

    fn cancel(&mut self) -> Result<(), HalError> {
        // The instance a cancelled power command had already put into an in-between state goes back
        // to what it was, or a page that walked away mid-command would come back to a machine stuck
        // at "starting" for ever.
        if let Some(request) = self.in_flight.take() {
            if request.action.is_power() {
                if let CloudTarget::Machine(product) = request.target {
                    if let Some(found) = self.instances[product.index()]
                        .iter_mut()
                        .find(|found| found.id == request.instance)
                    {
                        found.state = match request.action {
                            CloudAction::Start => CloudInstanceState::Stopped,
                            CloudAction::Stop => CloudInstanceState::Running,
                            _ => CloudInstanceState::Running,
                        };
                    }
                }
            }
        }

        self.answer = None;
        self.state = CloudState::Idle;
        Ok(())
    }

    fn tick(&mut self) {
        let Some(request) = self.in_flight.as_mut() else {
            return;
        };

        request.ticks_left = request.ticks_left.saturating_sub(1);
        if request.ticks_left > 0 {
            return;
        }

        let request = self.in_flight.take().expect("checked above");
        self.settle(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled(cloud: &mut SimCloud) {
        for _ in 0..LATENCY {
            cloud.tick();
        }
    }

    #[test]
    fn a_list_takes_its_ticks_and_then_answers() {
        let mut cloud = SimCloud::new();

        cloud
            .start(CloudTarget::Vendor(CloudVendor::Tencent), CloudAction::List, "", "")
            .unwrap();
        assert_eq!(cloud.state(), CloudState::Running);
        // Nothing to take while the request is in flight, and a page that tried is told so rather
        // than handed a half-built answer.
        assert_eq!(cloud.take_answer(), Err(HalError::Busy));

        settled(&mut cloud);
        assert_eq!(cloud.state(), CloudState::Done);

        let CloudAnswer::Instances {
            vendor,
            region,
            instances,
            ..
        } = cloud.take_answer().unwrap()
        else {
            panic!("a list should read as instances");
        };

        assert_eq!(vendor, CloudVendor::Tencent);
        // A list spans regions, so it names none.
        assert_eq!(region, "");
        // Both products of the vendor, merged.
        assert_eq!(instances.len(), 3);
        assert_eq!(instances[2].provider, CloudProvider::Lighthouse);
        // Taking it is what ends the request: the state goes back to idle and there is no second
        // answer to take.
        assert_eq!(cloud.state(), CloudState::Idle);
        assert_eq!(cloud.take_answer(), Err(HalError::Busy));
    }

    #[test]
    fn a_list_of_one_vendor_does_not_show_another_vendors_machines() {
        let mut cloud = SimCloud::new();

        cloud
            .start(CloudTarget::Vendor(CloudVendor::Aliyun), CloudAction::List, "", "")
            .unwrap();
        settled(&mut cloud);

        let CloudAnswer::Instances { vendor, instances, .. } = cloud.take_answer().unwrap() else {
            panic!("a list should read as instances");
        };

        assert_eq!(vendor, CloudVendor::Aliyun);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].provider, CloudProvider::Ecs);
        assert_eq!(instances[0].region, ALIYUN_REGION);
    }

    #[test]
    fn a_second_request_while_one_is_in_flight_is_busy() {
        let mut cloud = SimCloud::new();

        cloud
            .start(CloudTarget::Vendor(CloudVendor::Tencent), CloudAction::List, "", "")
            .unwrap();
        assert_eq!(
            cloud.start(CloudTarget::Vendor(CloudVendor::Tencent), CloudAction::List, "", ""),
            Err(HalError::Busy)
        );
    }

    #[test]
    fn a_power_command_flips_the_machine_it_names() {
        let mut cloud = SimCloud::new();

        cloud
            .start(
                CloudTarget::Machine(CloudProvider::Cvm),
                CloudAction::Start,
                "ins-e5f6a7b8",
                "",
            )
            .unwrap();
        // Busy before the provider has answered, which is what the row draws.
        assert!(cloud.instances[CloudProvider::Cvm.index()][1].state.is_busy());

        settled(&mut cloud);
        let CloudAnswer::Accepted {
            provider, action, ..
        } = cloud.take_answer().unwrap()
        else {
            panic!("a power command should read as an acceptance");
        };
        assert_eq!(provider, CloudProvider::Cvm);
        assert_eq!(action, CloudAction::Start);
        assert_eq!(
            cloud.instances[CloudProvider::Cvm.index()][1].state,
            CloudInstanceState::Running
        );
    }

    #[test]
    fn a_power_command_with_no_instance_to_aim_at_is_a_bad_argument() {
        let mut cloud = SimCloud::new();

        assert_eq!(
            cloud.start(CloudTarget::Machine(CloudProvider::Cvm), CloudAction::Stop, "", ""),
            Err(HalError::InvalidArg)
        );
    }

    #[test]
    fn a_power_command_that_names_a_vendor_is_a_bad_argument() {
        let mut cloud = SimCloud::new();

        assert_eq!(
            cloud.start(CloudTarget::Vendor(CloudVendor::Tencent), CloudAction::Stop, "ins-1", ""),
            Err(HalError::InvalidArg)
        );
    }

    #[test]
    fn a_list_that_names_a_product_is_a_bad_argument() {
        let mut cloud = SimCloud::new();

        assert_eq!(
            cloud.start(CloudTarget::Machine(CloudProvider::Cvm), CloudAction::List, "", ""),
            Err(HalError::InvalidArg)
        );
    }

    #[test]
    fn a_vendor_with_no_key_says_so_rather_than_failing_to_connect() {
        let mut cloud = SimCloud::new().without_key(CloudVendor::Aliyun);

        assert!(!cloud.key(CloudVendor::Aliyun).configured());
        cloud
            .start(CloudTarget::Vendor(CloudVendor::Aliyun), CloudAction::List, "", "")
            .unwrap();
        settled(&mut cloud);

        assert_eq!(cloud.state(), CloudState::Failed(CloudFault::NoCredentials));
        let CloudAnswer::Refused { fault, .. } = cloud.take_answer().unwrap() else {
            panic!("no key should read as a refusal");
        };
        assert_eq!(fault, CloudFault::NoCredentials);
    }

    #[test]
    fn aws_is_a_row_with_no_client_and_says_so() {
        let mut cloud = SimCloud::new();

        cloud
            .start(CloudTarget::Vendor(CloudVendor::Aws), CloudAction::List, "", "")
            .unwrap();
        settled(&mut cloud);

        // Not "no credentials", which would tell the person to type a key that cannot help yet.
        assert_eq!(cloud.state(), CloudState::Failed(CloudFault::Unsupported));
    }

    #[test]
    fn a_cancelled_command_leaves_the_machine_where_it_was() {
        let mut cloud = SimCloud::new();
        let before = cloud.instances[CloudProvider::Cvm.index()].clone();

        cloud
            .start(
                CloudTarget::Machine(CloudProvider::Cvm),
                CloudAction::Stop,
                "ins-a1b2c3d4",
                "",
            )
            .unwrap();
        cloud.cancel().unwrap();

        assert_eq!(cloud.state(), CloudState::Idle);
        assert_eq!(
            cloud.instances[CloudProvider::Cvm.index()][0].state,
            before[0].state
        );
        // And nothing of the cancelled request is left to arrive late.
        settled(&mut cloud);
        assert_eq!(cloud.state(), CloudState::Idle);
    }
}
