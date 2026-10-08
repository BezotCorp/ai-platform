use bcaip_sdk_types::custom_requests::{
    CreateScheduleRequest, CreateScheduleResponse, DeleteScheduleRequest, EmptyResponse,
    InspectRunningJobRequest, InspectRunningJobResponse, KillRunningJobRequest,
    KillRunningJobResponse, ListScheduleSessionsRequest, ListScheduleSessionsResponse,
    ListSchedulesRequest, ListSchedulesResponse, PauseScheduleRequest, RunScheduleNowRequest,
    RunScheduleNowResponse, RunScheduleNowStatus, ScheduledJobDto, UnpauseScheduleRequest,
    UpdateScheduleRequest, UpdateScheduleResponse,
};

use crate::acp::response_builder::build_session_info;
use crate::acp::server::server_informations::{BcaipAcpAgent, ResultExt};
use crate::recipe::{Recipe, validate_recipe::validate_recipe_template_from_content};
use crate::scheduler::{ScheduledJob, SchedulerError, ValidatedScheduleRecipe};
use crate::scheduler_trait::SchedulerTrait;
use std::sync::Arc;
fn validate_schedule_id(id: &str) -> Result<(), agent_client_protocol::Error> {
    let is_valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ');

    if !is_valid {
        return Err(agent_client_protocol::Error::invalid_params().data(
            "Schedule name must use only alphanumeric characters, hyphens, underscores, or spaces",
        ));
    }

    Ok(())
}

fn validate_schedule_recipe(recipe: &Recipe) -> Result<String, agent_client_protocol::Error> {
    let recipe_yaml = recipe
        .to_yaml()
        .map_err(|e| agent_client_protocol::Error::invalid_params().data(e.to_string()))?;

    validate_recipe_template_from_content(&recipe_yaml, None)
        .map_err(|e| agent_client_protocol::Error::invalid_params().data(e.to_string()))?;

    Ok(recipe_yaml)
}

fn schedule_not_found_or_internal(error: SchedulerError) -> agent_client_protocol::Error {
    match error {
        SchedulerError::JobNotFound(id) => {
            agent_client_protocol::Error::resource_not_found(Some(id))
        }
        error => agent_client_protocol::Error::internal_error().data(error.to_string()),
    }
}

fn create_schedule_error(error: SchedulerError) -> agent_client_protocol::Error {
    match error {
        SchedulerError::CronParseError(message) => agent_client_protocol::Error::invalid_params()
            .data(format!("Invalid cron expression: {message}")),
        SchedulerError::RecipeLoadError(message) => agent_client_protocol::Error::invalid_params()
            .data(format!("Recipe load error: {message}")),
        SchedulerError::JobIdExists(id) => agent_client_protocol::Error::invalid_params()
            .data(format!("Job ID already exists: {id}")),
        error => agent_client_protocol::Error::internal_error()
            .data(format!("Error creating schedule: {error}")),
    }
}

fn schedule_state_error(error: SchedulerError) -> agent_client_protocol::Error {
    match error {
        SchedulerError::JobNotFound(id) => {
            agent_client_protocol::Error::resource_not_found(Some(id))
        }
        SchedulerError::AnyhowError(error) => {
            agent_client_protocol::Error::invalid_params().data(error.to_string())
        }
        error => agent_client_protocol::Error::internal_error().data(error.to_string()),
    }
}

fn update_schedule_error(error: SchedulerError) -> agent_client_protocol::Error {
    match error {
        SchedulerError::JobNotFound(id) => {
            agent_client_protocol::Error::resource_not_found(Some(id))
        }
        SchedulerError::AnyhowError(error) => {
            agent_client_protocol::Error::invalid_params().data(error.to_string())
        }
        SchedulerError::CronParseError(message) => agent_client_protocol::Error::invalid_params()
            .data(format!("Invalid cron expression: {message}")),
        error => agent_client_protocol::Error::internal_error().data(error.to_string()),
    }
}

fn run_schedule_now_error(
    error: SchedulerError,
) -> Result<RunScheduleNowResponse, agent_client_protocol::Error> {
    match error {
        SchedulerError::JobNotFound(id) => {
            Err(agent_client_protocol::Error::resource_not_found(Some(id)))
        }
        SchedulerError::AnyhowError(error)
            if error.to_string().contains("was successfully cancelled") =>
        {
            Ok(RunScheduleNowResponse {
                status: RunScheduleNowStatus::Cancelled,
                session_id: None,
            })
        }
        error => Err(agent_client_protocol::Error::internal_error()
            .data(format!("Error running schedule: {error}"))),
    }
}

fn scheduled_job_to_dto(job: ScheduledJob) -> ScheduledJobDto {
    ScheduledJobDto {
        id: job.id,
        source: job.source,
        cron: job.cron,
        last_run: job.last_run.map(|value| value.to_rfc3339()),
        currently_running: job.currently_running,
        paused: job.paused,
        current_session_id: job.current_session_id,
        job_start_time: job.process_start_time.map(|value| value.to_rfc3339()),
    }
}

impl BcaipAcpAgent {
    pub(crate) fn require_scheduler(
        &self,
    ) -> Result<Arc<dyn SchedulerTrait>, agent_client_protocol::Error> {
        self.agent_manager().scheduler().ok_or_else(|| {
            agent_client_protocol::Error::method_not_found()
                .data("Scheduled recipe execution is not enabled")
        })
    }

    pub(crate) async fn on_list_schedules(
        &self,
        _req: ListSchedulesRequest,
    ) -> Result<ListSchedulesResponse, agent_client_protocol::Error> {
        let jobs = self
            .require_scheduler()?
            .list_scheduled_jobs()
            .await
            .into_iter()
            .map(scheduled_job_to_dto)
            .collect();

        Ok(ListSchedulesResponse { jobs })
    }

    pub(crate) async fn on_list_schedule_sessions(
        &self,
        req: ListScheduleSessionsRequest,
    ) -> Result<ListScheduleSessionsResponse, agent_client_protocol::Error> {
        let sessions = self
            .require_scheduler()?
            .sessions(&req.schedule_id, req.limit)
            .await
            .internal_err_ctx("Failed to fetch schedule sessions")?
            .into_iter()
            .map(|(_, session)| build_session_info(session))
            .collect();

        Ok(ListScheduleSessionsResponse { sessions })
    }

    pub(crate) async fn on_create_schedule(
        &self,
        req: CreateScheduleRequest,
    ) -> Result<CreateScheduleResponse, agent_client_protocol::Error> {
        let scheduler = self.require_scheduler()?;
        let id = req.id.trim().to_string();
        validate_schedule_id(&id)?;

        let recipe = Recipe::try_from(req.recipe).map_err(|e| {
            agent_client_protocol::Error::invalid_params().data(format!("recipe: {e}"))
        })?;

        if recipe.check_for_security_warnings() {
            return Err(agent_client_protocol::Error::invalid_params().data(
                "This recipe contains hidden characters that could be malicious. Please remove them before trying to save.",
            ));
        }
        let yaml_content = validate_schedule_recipe(&recipe)?;
        let recipe_source = format!("{id}.yaml");

        let job = ScheduledJob {
            id: id.clone(),
            source: recipe_source.clone(),
            cron: req.cron,
            last_run: None,
            currently_running: false,
            paused: false,
            current_session_id: None,
            process_start_time: None,
            parameters: vec![],
            recipe_base_dir: None,
        };

        scheduler
            .add_scheduled_job_with_recipe(
                job,
                ValidatedScheduleRecipe::new(yaml_content.into_bytes(), recipe_source.into()),
            )
            .await
            .map_err(create_schedule_error)?;

        let job = scheduler
            .list_scheduled_jobs()
            .await
            .into_iter()
            .find(|job| job.id == id)
            .ok_or_else(|| {
                agent_client_protocol::Error::internal_error()
                    .data("Schedule not found after creation")
            })?;

        Ok(CreateScheduleResponse {
            job: scheduled_job_to_dto(job),
        })
    }

    pub(crate) async fn on_delete_schedule(
        &self,
        req: DeleteScheduleRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        self.require_scheduler()?
            .remove_scheduled_job(&req.schedule_id, false)
            .await
            .map_err(schedule_not_found_or_internal)?;

        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_pause_schedule(
        &self,
        req: PauseScheduleRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        self.require_scheduler()?
            .pause_schedule(&req.schedule_id)
            .await
            .map_err(schedule_state_error)?;

        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_unpause_schedule(
        &self,
        req: UnpauseScheduleRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        self.require_scheduler()?
            .unpause_schedule(&req.schedule_id)
            .await
            .map_err(schedule_not_found_or_internal)?;

        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_update_schedule(
        &self,
        req: UpdateScheduleRequest,
    ) -> Result<UpdateScheduleResponse, agent_client_protocol::Error> {
        let schedule_id = req.schedule_id;
        let cron = req.cron;
        let scheduler = self.require_scheduler()?;
        scheduler
            .update_schedule(&schedule_id, cron)
            .await
            .map_err(update_schedule_error)?;

        let job = scheduler
            .list_scheduled_jobs()
            .await
            .into_iter()
            .find(|job| job.id == schedule_id)
            .ok_or_else(|| {
                agent_client_protocol::Error::internal_error()
                    .data("Schedule not found after update")
            })?;

        Ok(UpdateScheduleResponse {
            job: scheduled_job_to_dto(job),
        })
    }

    pub(crate) async fn on_run_schedule_now(
        &self,
        req: RunScheduleNowRequest,
    ) -> Result<RunScheduleNowResponse, agent_client_protocol::Error> {
        match self.require_scheduler()?.run_now(&req.schedule_id).await {
            Ok(session_id) => Ok(RunScheduleNowResponse {
                status: RunScheduleNowStatus::Completed,
                session_id: Some(session_id),
            }),
            Err(error) => run_schedule_now_error(error),
        }
    }

    pub(crate) async fn on_kill_running_job(
        &self,
        req: KillRunningJobRequest,
    ) -> Result<KillRunningJobResponse, agent_client_protocol::Error> {
        self.require_scheduler()?
            .kill_running_job(&req.job_id)
            .await
            .map_err(schedule_state_error)?;

        Ok(KillRunningJobResponse {
            message: format!("Successfully killed running job '{}'", req.job_id),
        })
    }

    pub(crate) async fn on_inspect_running_job(
        &self,
        req: InspectRunningJobRequest,
    ) -> Result<InspectRunningJobResponse, agent_client_protocol::Error> {
        let job = self
            .require_scheduler()?
            .list_scheduled_jobs()
            .await
            .into_iter()
            .find(|job| job.id == req.job_id)
            .ok_or_else(|| agent_client_protocol::Error::resource_not_found(Some(req.job_id)))?;

        if !job.currently_running {
            return Ok(InspectRunningJobResponse::default());
        }

        let running_duration_seconds = job.process_start_time.map(|start_time| {
            chrono::Utc::now()
                .signed_duration_since(start_time)
                .num_seconds()
        });

        Ok(InspectRunningJobResponse {
            running: true,
            session_id: job.current_session_id,
            job_start_time: job.process_start_time.map(|value| value.to_rfc3339()),
            running_duration_seconds,
        })
    }
}
