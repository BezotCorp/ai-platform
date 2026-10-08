use crate::acp::server::server_informations::{BcaipAcpAgent, ResultExt};
use crate::session::{DiagnosticsLevel, generate_diagnostics};
use bcaip_sdk_types::custom_requests::{
    DiagnosticsGetRequest, DiagnosticsGetResponse, DiagnosticsReportLevel,
};

impl BcaipAcpAgent {
    pub(super) async fn on_get_diagnostics(
        &self,
        req: DiagnosticsGetRequest,
    ) -> Result<DiagnosticsGetResponse, agent_client_protocol::Error> {
        let level = match req.level {
            DiagnosticsReportLevel::Summary => DiagnosticsLevel::Summary,
            DiagnosticsReportLevel::Full => DiagnosticsLevel::Full,
        };
        let report = generate_diagnostics(self.session_manager(), &req.session_id, level)
            .await
            .internal_err()?;
        let report = serde_json::to_value(report).internal_err()?;

        Ok(DiagnosticsGetResponse { report })
    }
}
