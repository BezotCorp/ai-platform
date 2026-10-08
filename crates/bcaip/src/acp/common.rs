use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome,
};
use bcaip_provider_types::permission::Permission;
use std::str::FromStr;
use strum::{Display, EnumString};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum PermissionDecision {
    AllowAlways,
    AllowOnce,
    RejectAlways,
    RejectOnce,
    Cancel,
}

impl PermissionDecision {
    pub fn should_record_rejection(self) -> bool {
        matches!(
            self,
            PermissionDecision::RejectAlways
                | PermissionDecision::RejectOnce
                | PermissionDecision::Cancel
        )
    }
}

impl From<Permission> for PermissionDecision {
    fn from(p: Permission) -> Self {
        match p {
            Permission::AlwaysAllow => Self::AllowAlways,
            Permission::AllowOnce => Self::AllowOnce,
            Permission::DenyOnce => Self::RejectOnce,
            Permission::AlwaysDeny => Self::RejectAlways,
            Permission::Cancel => Self::Cancel,
        }
    }
}

impl From<PermissionDecision> for Permission {
    fn from(d: PermissionDecision) -> Self {
        match d {
            PermissionDecision::AllowAlways => Self::AlwaysAllow,
            PermissionDecision::AllowOnce => Self::AllowOnce,
            PermissionDecision::RejectOnce => Self::DenyOnce,
            PermissionDecision::RejectAlways => Self::AlwaysDeny,
            PermissionDecision::Cancel => Self::Cancel,
        }
    }
}

impl From<&RequestPermissionOutcome> for PermissionDecision {
    fn from(outcome: &RequestPermissionOutcome) -> Self {
        match outcome {
            RequestPermissionOutcome::Cancelled => Self::Cancel,
            RequestPermissionOutcome::Selected(selected) => {
                Self::from_str(&selected.option_id.0).unwrap_or(Self::Cancel)
            }
            _ => Self::Cancel,
        }
    }
}

/// Map a permission decision to a response by matching the option kind from the
/// request. A decision may fall back only when the alternative does not increase
/// the granted permission scope (e.g. AllowAlways falls back to AllowOnce).
pub fn map_permission_response(
    request: &RequestPermissionRequest,
    decision: PermissionDecision,
) -> RequestPermissionResponse {
    let selected_id = match decision {
        PermissionDecision::AllowAlways => {
            find_option(&request.options, PermissionOptionKind::AllowAlways)
                .or_else(|| find_option(&request.options, PermissionOptionKind::AllowOnce))
        }
        PermissionDecision::AllowOnce => {
            find_option(&request.options, PermissionOptionKind::AllowOnce)
        }
        PermissionDecision::RejectAlways => {
            find_option(&request.options, PermissionOptionKind::RejectAlways)
                .or_else(|| find_option(&request.options, PermissionOptionKind::RejectOnce))
        }
        PermissionDecision::RejectOnce => {
            find_option(&request.options, PermissionOptionKind::RejectOnce)
                .or_else(|| find_option(&request.options, PermissionOptionKind::RejectAlways))
        }
        PermissionDecision::Cancel => None,
    };

    if let Some(option_id) = selected_id {
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(option_id),
        ))
    } else {
        RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
    }
}

fn find_option(options: &[PermissionOption], kind: PermissionOptionKind) -> Option<String> {
    options
        .iter()
        .find(|opt| opt.kind == kind)
        .map(|opt| opt.option_id.0.to_string())
}
