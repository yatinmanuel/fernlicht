use std::time::Duration;

use super::identify::Profile;
use super::tables::{FLE_ROUTINE, did};
use crate::bytes::be_u16;
use crate::transport::UdsLink;
use crate::uds::{Reply, Routine, Session, sid};
use crate::{Error, Result};

/// Wraps a link so that only reads and the known light commands get through,
/// and the light commands only to modules the profile identified.
///
/// Anything built from user input (shows, scripts, experiments) belongs
/// behind a guard.
#[derive(Debug, Clone)]
pub struct Guard<L> {
    inner: L,
    profile: Profile,
}

impl<L: UdsLink> Guard<L> {
    pub fn new(inner: L, profile: Profile) -> Self {
        Self { inner, profile }
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn into_inner(self) -> L {
        self.inner
    }

    fn allows(&self, ecu: u16, request: &[u8]) -> bool {
        let Some(&service) = request.first() else {
            return false;
        };
        let sub = request.get(1).copied();
        match service {
            sid::READ_DID | sid::TESTER_PRESENT => return true,
            sid::SESSION_CONTROL => {
                return sub == Some(Session::Default as u8) || sub == Some(Session::Extended as u8);
            }
            _ => {}
        }

        let profile = &self.profile;
        let id = be_u16(request, 1);
        if Some(ecu) == profile.body {
            (service == sid::WRITE_DID && id == Some(did::LAMP_FUNCTION))
                || (service == sid::IO_CONTROL && id == Some(did::LAMP_OUTPUT))
        } else if Some(ecu) == profile.left || Some(ecu) == profile.right {
            service == sid::ROUTINE_CONTROL
                && (sub == Some(Routine::Start as u8) || sub == Some(Routine::Stop as u8))
                && be_u16(request, 2) == Some(FLE_ROUTINE)
        } else if Some(ecu) == profile.rear {
            service == sid::WRITE_DID && id == Some(did::LAMP_OUTPUT)
        } else {
            false
        }
    }
}

impl<L: UdsLink> UdsLink for Guard<L> {
    fn request(&self, ecu: u16, request: &[u8], timeout: Duration) -> Result<Reply> {
        if !self.allows(ecu, request) {
            return Err(Error::Blocked { ecu, request: request.to_vec() });
        }
        self.inner.request(ecu, request, timeout)
    }
}
