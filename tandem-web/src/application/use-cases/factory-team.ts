import {
  FACTORY_CLIS,
  FactoryGatewayError,
  isValidAgentJobId,
  isValidAgentMessage,
  isValidCli,
  isValidModelName,
  isValidRunId,
  isValidStageName,
  type RunAgentsPayload,
  type RelaunchAgentResult,
  type StageAssignment,
} from "@/domain/models/factory";
import type { ITandemGateway } from "@/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "@/domain/ports/workspace-registry";

async function requireWorkspace(registry: IWorkspaceRegistry, workspaceId: string) {
  const workspace = await registry.findById(workspaceId);
  if (!workspace) {
    throw new FactoryGatewayError("not_found", "Workspace not found");
  }
  return workspace;
}

export class GetRunAgentsUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, runId: string): Promise<RunAgentsPayload> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    const [agents, assignments] = await Promise.all([
      this.gateway.factoryListAgents
        ? this.gateway.factoryListAgents(workspace.path, runId)
        : Promise.resolve([]),
      this.gateway.factoryGetAssignments
        ? this.gateway.factoryGetAssignments(workspace.path, runId)
        : Promise.resolve({}),
    ]);
    return { run_id: runId, agents, assignments };
  }
}

export class AssignStageUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    params: { stage: string; cli: string; model: string; role?: string }
  ): Promise<StageAssignment> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidStageName(params.stage)) {
      throw new FactoryGatewayError("invalid_input", "Invalid stage name");
    }
    if (!isValidCli(params.cli)) {
      throw new FactoryGatewayError(
        "invalid_input",
        `Unsupported CLI "${params.cli}" (expected ${FACTORY_CLIS.join(", ")})`
      );
    }
    if (!isValidModelName(params.model)) {
      throw new FactoryGatewayError("invalid_input", "Invalid model name");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    if (!this.gateway.factoryAssignStage) {
      throw new FactoryGatewayError("cli_error", "Agent assignment unavailable");
    }
    return this.gateway.factoryAssignStage(workspace.path, runId, {
      stage: params.stage,
      cli: params.cli,
      model: params.model.trim(),
      role: params.role?.trim() || undefined,
    });
  }
}

export class SendAgentMessageUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    jobId: string,
    message: string
  ): Promise<void> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidAgentJobId(jobId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid job ID format");
    }
    if (!isValidAgentMessage(message)) {
      throw new FactoryGatewayError(
        "invalid_input",
        "Message must be non-empty and at most 64 KiB"
      );
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    if (!this.gateway.factorySendAgentMessage) {
      throw new FactoryGatewayError("cli_error", "Agent messaging unavailable");
    }
    await this.gateway.factorySendAgentMessage(workspace.path, runId, jobId, message);
  }
}

export class TerminateAgentUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    jobId: string,
    confirm: string
  ): Promise<void> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidAgentJobId(jobId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid job ID format");
    }
    if (confirm !== jobId) {
      throw new FactoryGatewayError("invalid_input", `Confirmation must match "${jobId}"`);
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    if (!this.gateway.factoryTerminateAgent) {
      throw new FactoryGatewayError("cli_error", "Agent termination unavailable");
    }
    await this.gateway.factoryTerminateAgent(workspace.path, runId, jobId, confirm);
  }
}

export class RelaunchAgentUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    jobId: string
  ): Promise<RelaunchAgentResult> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidAgentJobId(jobId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid job ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    if (!this.gateway.factoryRelaunchAgent) {
      throw new FactoryGatewayError("cli_error", "Agent relaunch unavailable");
    }
    return this.gateway.factoryRelaunchAgent(workspace.path, runId, jobId);
  }
}
