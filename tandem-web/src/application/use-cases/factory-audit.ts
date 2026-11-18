import {
  FactoryGatewayError,
  isValidAuditActor,
  isValidAuditKind,
  isValidIdeaId,
  isValidRunId,
  type AuditFilterOptions,
  type AuditTimelinePayload,
} from "../../domain/models/factory";
import { isValidWorkspaceId, type WorkspaceEntry } from "../../domain/models/workspace";
import type { ITandemGateway } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

async function requireWorkspace(
  registry: IWorkspaceRegistry,
  workspaceId: string
): Promise<WorkspaceEntry> {
  if (!isValidWorkspaceId(workspaceId)) {
    throw new FactoryGatewayError(
      "invalid_input",
      `Invalid workspace ID format: '${workspaceId}'`
    );
  }
  const workspace = await registry.findById(workspaceId);
  if (!workspace) {
    throw new FactoryGatewayError("not_found", `Workspace not found: '${workspaceId}'`);
  }
  return workspace;
}

export interface RawAuditFilters {
  run?: string;
  kind?: string;
  actor?: string;
}

function validateFilters(raw: RawAuditFilters): AuditFilterOptions {
  const out: AuditFilterOptions = {};
  if (raw.run && raw.run !== "all") {
    if (!isValidRunId(raw.run)) {
      throw new FactoryGatewayError("invalid_input", `Invalid run ID format: '${raw.run}'`);
    }
    out.run = raw.run;
  }
  if (raw.kind && raw.kind !== "all") {
    if (!isValidAuditKind(raw.kind)) {
      throw new FactoryGatewayError("invalid_input", `Invalid audit kind: '${raw.kind}'`);
    }
    out.kind = raw.kind;
  }
  if (raw.actor && raw.actor !== "all") {
    if (!isValidAuditActor(raw.actor)) {
      throw new FactoryGatewayError("invalid_input", `Invalid audit actor: '${raw.actor}'`);
    }
    out.actor = raw.actor.trim();
  }
  return out;
}

export class GetFactoryIdeaAuditUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    ideaId: string,
    rawFilters: RawAuditFilters = {}
  ): Promise<AuditTimelinePayload> {
    if (!isValidIdeaId(ideaId)) {
      throw new FactoryGatewayError("invalid_input", `Invalid idea ID format: '${ideaId}'`);
    }
    const filters = validateFilters(rawFilters);
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryAuditIdea(workspace.path, ideaId, filters);
  }

  async executeMarkdown(
    workspaceId: string,
    ideaId: string,
    rawFilters: RawAuditFilters = {}
  ): Promise<string> {
    if (!isValidIdeaId(ideaId)) {
      throw new FactoryGatewayError("invalid_input", `Invalid idea ID format: '${ideaId}'`);
    }
    const filters = validateFilters(rawFilters);
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryAuditMarkdown(workspace.path, ideaId, filters);
  }
}
