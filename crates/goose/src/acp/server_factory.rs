use crate::acp::server::{
    AcpBuiltinSelection, AcpProviderFactory, ActiveRunRegistry, GooseAcpAgent,
    GooseAcpAgentOptions, LiveVoiceService,
};
#[cfg(feature = "scheduler")]
use crate::scheduler_trait::SchedulerTrait;
#[cfg(feature = "scheduler")]
use crate::session::SessionManager;
use crate::source_roots::SourceRoot;
use crate::{agents::GoosePlatform, config::paths::Paths};
use anyhow::Result;
use std::sync::Arc;
#[cfg(feature = "scheduler")]
use tokio::sync::OnceCell;
use tracing::info;
pub struct AcpServerFactoryConfig {
    pub builtins: AcpBuiltinSelection,
    pub config_dir: std::path::PathBuf,
    pub goose_platform: GoosePlatform,
    pub additional_source_roots: Vec<SourceRoot>,
    /// When set, new sessions use this host-controlled working directory
    /// instead of the `cwd` the connecting client sends. Used by roaming, where
    /// the connector's absolute path is meaningless on the host machine.
    pub session_cwd: Option<std::path::PathBuf>,
    pub enable_scheduler: bool,
}

pub struct AcpServer {
    config: AcpServerFactoryConfig,
    data_dir: std::path::PathBuf,
    #[cfg(feature = "scheduler")]
    scheduler: OnceCell<Arc<dyn SchedulerTrait>>,
    active_runs: Arc<ActiveRunRegistry>,
    live_voice: Arc<crate::acp::server::LiveVoiceService>,
}

impl AcpServer {
    pub fn new(config: AcpServerFactoryConfig) -> Self {
        let data_dir = Paths::data_dir();
        let active_runs = Arc::new(ActiveRunRegistry::default());
        let live_voice = Arc::new(LiveVoiceService::from_config(active_runs.clone()));
        Self {
            config,
            data_dir,
            #[cfg(feature = "scheduler")]
            scheduler: OnceCell::new(),
            active_runs,
            live_voice,
        }
    }

    #[cfg(feature = "scheduler")]
    /// Start the scheduler now instead of on first client connect, so a
    /// headless `goose serve` runs scheduled jobs; on failure `create_agent`
    /// retries. No-op when the scheduler is disabled.
    pub async fn start_scheduler(&self) -> Result<()> {
        self.scheduler().await.map(|_| ())
    }

    #[cfg(feature = "scheduler")]
    async fn scheduler(&self) -> Result<Option<Arc<dyn SchedulerTrait>>> {
        if !self.config.enable_scheduler {
            return Ok(None);
        }

        let data_dir = self.data_dir.clone();
        self.scheduler
            .get_or_try_init(|| async move {
                let session_manager = Arc::new(SessionManager::new(data_dir.clone()));
                let schedule_file_path = data_dir.join("schedule.json");
                let scheduler =
                    crate::scheduler::Scheduler::new(schedule_file_path, session_manager)
                        .await
                        .map(|scheduler| scheduler as Arc<dyn SchedulerTrait>)?;
                Ok(scheduler)
            })
            .await
            .cloned()
            .map(Some)
    }

    pub async fn create_agent(&self) -> Result<Arc<GooseAcpAgent>> {
        self.create_agent_with_session_cwd(self.config.session_cwd.clone())
            .await
    }

    /// Create an agent whose sessions use `session_cwd` instead of this
    /// server's configured default. Used by the roaming bridge on `goose
    /// serve --roam`: the serve-wide server keeps `session_cwd: None` for
    /// local ACP clients whose paths are real on this machine, while each
    /// roaming connection gets a host-controlled working directory (the
    /// connector's absolute path is meaningless here). The agent still shares
    /// this server's active-run registry.
    pub async fn create_agent_with_session_cwd(
        &self,
        session_cwd: Option<std::path::PathBuf>,
    ) -> Result<Arc<GooseAcpAgent>> {
        let config = crate::config::Config::global();
        let disable_session_naming = config.get_goose_disable_session_naming().unwrap_or(false);
        #[cfg(feature = "scheduler")]
        let scheduler = self.scheduler().await?;
        #[cfg(feature = "scheduler")]
        if let Some(scheduler) = &scheduler {
            scheduler.list_scheduled_jobs().await;
        }

        let provider_factory: AcpProviderFactory = Arc::new(
            move |provider_name, extensions, working_dir, use_default_model| {
                Box::pin(async move {
                    if use_default_model {
                        crate::providers::create_with_default_model(&provider_name, extensions)
                            .await
                    } else {
                        match working_dir {
                            Some(working_dir) => {
                                crate::providers::create_with_working_dir(
                                    &provider_name,
                                    extensions,
                                    working_dir,
                                )
                                .await
                            }
                            None => crate::providers::create(&provider_name, extensions).await,
                        }
                    }
                })
            },
        );

        let agent = GooseAcpAgent::new(GooseAcpAgentOptions {
            provider_factory,
            builtin_selection: self.config.builtins.clone(),
            data_dir: self.data_dir.clone(),
            config_dir: self.config.config_dir.clone(),
            disable_session_naming,
            goose_platform: self.config.goose_platform.clone(),
            additional_source_roots: self.config.additional_source_roots.clone(),
            session_cwd,
            #[cfg(feature = "scheduler")]
            scheduler,
            #[cfg(not(feature = "scheduler"))]
            scheduler: None,
            active_runs: self.active_runs.clone(),
            live_voice: self.live_voice.clone(),
        })
        .await?;
        info!("Created new ACP agent");

        Ok(Arc::new(agent))
    }
}
