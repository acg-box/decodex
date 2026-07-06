use std::time::Duration;

use crate::{
	agent::app_server::{
		REQUEST_TIMEOUT,
		protocol::{
			ConfigReadParams, ConfigReadResponse, ListMcpServerStatusParams,
			ListMcpServerStatusResponse, ModelListParams, ModelListResponse,
			ModelProviderCapabilitiesReadParams, ModelProviderCapabilitiesReadResponse,
			PluginListParams, PluginListResponse, SkillsListParams, SkillsListResponse,
			client::AppServerClient,
		},
	},
	prelude::Result,
};

impl AppServerClient {
	pub(in crate::agent::app_server) fn read_config(
		&mut self,
		params: &ConfigReadParams,
	) -> Result<ConfigReadResponse> {
		self.connection.request("config/read", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn list_models(
		&mut self,
		params: &ModelListParams,
	) -> Result<ModelListResponse> {
		self.connection.request("model/list", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn read_model_provider_capabilities(
		&mut self,
	) -> Result<ModelProviderCapabilitiesReadResponse> {
		self.connection.request(
			"modelProvider/capabilities/read",
			&ModelProviderCapabilitiesReadParams {},
			REQUEST_TIMEOUT,
		)
	}

	pub(in crate::agent::app_server) fn list_skills(
		&mut self,
		params: &SkillsListParams,
	) -> Result<SkillsListResponse> {
		self.connection.request("skills/list", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn list_plugins(
		&mut self,
		params: &PluginListParams,
	) -> Result<PluginListResponse> {
		self.connection.request("plugin/list", params, REQUEST_TIMEOUT)
	}

	pub(in crate::agent::app_server) fn list_mcp_server_status(
		&mut self,
		params: &ListMcpServerStatusParams,
		timeout: Duration,
	) -> Result<ListMcpServerStatusResponse> {
		self.connection.request("mcpServerStatus/list", params, timeout)
	}
}
