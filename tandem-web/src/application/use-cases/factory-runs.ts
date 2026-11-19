import {
  FactoryGatewayError,
  isPngSignature,
  isValidCorrelationId,
  isValidEvidenceItemId,
  isValidHypothesis,
  isValidLogTailBytes,
  isValidRunControlAction,
  isValidRunId,
  isValidStageName,
  type EvidenceItemContent,
  type PrLink,
  type ProviderStatusItem,
  type RunControlOptions,
  type RunDataView,
  type RunEvidenceBundle,
  type RunSummary,
} from "@/domain/models/factory";
import type { ITandemGateway } from "@/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "@/domain/ports/workspace-registry";

const DEFAULT_LOG_TAIL_BYTES = 65_536;

async function requireWorkspace(registry: IWorkspaceRegistry, workspaceId: string) {
  const workspace = await registry.findById(workspaceId);
  if (!workspace) {
    throw new FactoryGatewayError("not_found", "Workspace not found");
  }
  return workspace;
}

export class GetFactoryRunDataUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, runId: string): Promise<RunDataView> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryRunData(workspace.path, runId);
  }
}

export class ShowFactoryRunUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, runId: string): Promise<RunSummary> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryShowRun(workspace.path, runId);
  }
}

export class GetFactoryRunLogUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    correlationId: string,
    bytes?: number
  ): Promise<string> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidCorrelationId(correlationId) || !correlationId.startsWith(`${runId}.`)) {
      throw new FactoryGatewayError("invalid_input", "Invalid job correlation ID");
    }
    const tailBytes = bytes === undefined ? DEFAULT_LOG_TAIL_BYTES : bytes;
    if (!isValidLogTailBytes(tailBytes)) {
      throw new FactoryGatewayError(
        "invalid_input",
        "Tail bytes must be an integer between 1 and 262144"
      );
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryRunLog(workspace.path, runId, correlationId, tailBytes);
  }
}

export class ControlFactoryRunUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    action: string,
    options?: RunControlOptions
  ): Promise<RunSummary> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidRunControlAction(action)) {
      throw new FactoryGatewayError("invalid_input", "Unknown run control action");
    }

    const sanitizedOptions: RunControlOptions = {};
    if (action === "pause" && options?.now) {
      sanitizedOptions.now = true;
    } else if (action === "cancel") {
      if (options?.confirm !== runId) {
        throw new FactoryGatewayError("invalid_input", "Confirmation must match run ID");
      }
      sanitizedOptions.confirm = options.confirm;
    } else if (action === "retry") {
      if (typeof options?.stage !== "string" || !isValidStageName(options.stage)) {
        throw new FactoryGatewayError("invalid_input", "Invalid stage name");
      }
      if (typeof options?.hypothesis !== "string" || !isValidHypothesis(options.hypothesis)) {
        throw new FactoryGatewayError(
          "invalid_input",
          "Hypothesis must be 1 to 500 characters without control characters"
        );
      }
      sanitizedOptions.stage = options.stage;
      sanitizedOptions.hypothesis = options.hypothesis.trim();
    }

    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryRunControl(workspace.path, runId, action, sanitizedOptions);
  }
}

export class ListFactoryProvidersUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<ProviderStatusItem[]> {
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryProviderStatus(workspace.path);
  }
}

export class GetFactoryRunEvidenceUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, runId: string): Promise<RunEvidenceBundle> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryRunEvidence(workspace.path, runId);
  }
}

export class GetFactoryEvidenceItemUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    runId: string,
    itemId: string
  ): Promise<EvidenceItemContent> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    if (!isValidEvidenceItemId(itemId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid evidence item ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    const result = await this.gateway.factoryEvidenceItem(workspace.path, runId, itemId);
    if (result.item.kind === "screenshot") {
      if (!result.pngBytes || !isPngSignature(result.pngBytes)) {
        throw new FactoryGatewayError("invalid_input", "Screenshot file is not a valid PNG");
      }
    }
    return result;
  }
}

export class RefreshFactoryRunPrsUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, runId: string): Promise<PrLink[]> {
    if (!isValidRunId(runId)) {
      throw new FactoryGatewayError("invalid_input", "Invalid run ID format");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryRefreshRunPrs(workspace.path, runId);
  }
}
