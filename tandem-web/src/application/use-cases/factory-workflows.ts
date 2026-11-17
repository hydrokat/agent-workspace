import {
  FactoryGatewayError,
  isValidRunId,
  type RunSummary,
  type WorkflowListItem,
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

export class ListFactoryWorkflowsUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<WorkflowListItem[]> {
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryListWorkflows(workspace.path);
  }
}

export class WithdrawFactoryRunUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, runId: string, confirm: string): Promise<RunSummary> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (confirm !== runId) {
      throw new FactoryGatewayError("invalid_input", "Confirmation must match run ID");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryWithdrawRun(workspace.path, runId);
  }
}
