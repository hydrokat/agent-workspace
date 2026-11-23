import {
  FactoryGatewayError,
  isValidArtifactKind,
  isValidArtifactVersion,
  isValidCli,
  isValidIdeaId,
  isValidIdeaText,
  isValidIdeaTitle,
  isValidModelIdForCli,
  isValidTargetRepo,
  isValidWorkflowName,
  type CancelIdeaResult,
  type Idea,
  type IdeaDetail,
  type ModelOption,
  type ReadyResult,
} from "../../domain/models/factory";
import { isValidWorkspaceId, type WorkspaceEntry } from "../../domain/models/workspace";
import type { ITandemGateway } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

async function requireWorkspace(
  registry: IWorkspaceRegistry,
  workspaceId: string
): Promise<WorkspaceEntry> {
  if (!isValidWorkspaceId(workspaceId)) {
    throw new FactoryGatewayError("invalid_input", `Invalid workspace ID format: '${workspaceId}'`);
  }
  const workspace = await registry.findById(workspaceId);
  if (!workspace) {
    throw new FactoryGatewayError("not_found", `Workspace not found: '${workspaceId}'`);
  }
  return workspace;
}

function requireIdeaId(ideaId: string): void {
  if (!isValidIdeaId(ideaId)) {
    throw new FactoryGatewayError("invalid_input", `Invalid idea ID format: '${ideaId}'`);
  }
}

export class ListFactoryIdeasUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<Idea[]> {
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryListIdeas(workspace.path);
  }
}

export class ShowFactoryIdeaUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, ideaId: string): Promise<IdeaDetail> {
    requireIdeaId(ideaId);
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryShowIdea(workspace.path, ideaId);
  }
}

export class ReadFactoryArtifactUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    ideaId: string,
    kind: string,
    version?: number
  ): Promise<string> {
    requireIdeaId(ideaId);
    if (!isValidArtifactKind(kind)) {
      throw new FactoryGatewayError("invalid_input", `Invalid artifact kind: '${kind}'`);
    }
    if (version !== undefined && !isValidArtifactVersion(version)) {
      throw new FactoryGatewayError("invalid_input", `Invalid artifact version: '${version}'`);
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryReadArtifact(workspace.path, ideaId, kind, version);
  }
}

export interface StartFactoryIdeaInput {
  cli: string;
  model: string;
  title?: string;
  text?: string;
}

export class StartFactoryIdeaUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, input: StartFactoryIdeaInput): Promise<Idea> {
    if (!isValidCli(input.cli)) {
      throw new FactoryGatewayError("invalid_input", `Invalid CLI provider: '${input.cli}'`);
    }
    if (!isValidModelIdForCli(input.cli, input.model)) {
      throw new FactoryGatewayError("invalid_input", `Invalid model ID: '${input.model}'`);
    }
    let normalizedTitle: string | undefined;
    if (input.title !== undefined) {
      if (!isValidIdeaTitle(input.title)) {
        throw new FactoryGatewayError("invalid_input", "Invalid idea title");
      }
      normalizedTitle = input.title.trim();
    }
    if (input.text !== undefined && !isValidIdeaText(input.text)) {
      throw new FactoryGatewayError("invalid_input", "Invalid idea description");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return input.text === undefined
      ? this.gateway.factoryStartIdea(workspace.path, input.cli, input.model, normalizedTitle)
      : this.gateway.factoryStartIdea(
          workspace.path,
          input.cli,
          input.model,
          normalizedTitle,
          input.text
        );
  }
}

export class ListFactoryModelsUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, cli: string): Promise<ModelOption[]> {
    if (!isValidCli(cli)) {
      throw new FactoryGatewayError("invalid_input", `Invalid CLI provider: '${cli}'`);
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryListModels(workspace.path, cli);
  }
}

export class MarkFactoryIdeaReadyUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    ideaId: string,
    confirm: string,
    options?: { workflow?: string; deploy?: boolean; target?: string }
  ): Promise<ReadyResult> {
    requireIdeaId(ideaId);
    if (confirm !== ideaId) {
      throw new FactoryGatewayError("invalid_input", "Confirmation must match idea ID");
    }
    if (options?.workflow !== undefined && !isValidWorkflowName(options.workflow)) {
      throw new FactoryGatewayError("invalid_input", "Invalid workflow name");
    }
    if (options?.target !== undefined && !isValidTargetRepo(options.target)) {
      throw new FactoryGatewayError("invalid_input", "Invalid target repository name");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryMarkReady(workspace.path, ideaId, options);
  }
}

export class CancelFactoryIdeaUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, ideaId: string, confirm: string): Promise<CancelIdeaResult> {
    requireIdeaId(ideaId);
    if (confirm !== ideaId) {
      throw new FactoryGatewayError("invalid_input", "Confirmation must match idea ID");
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    if (!this.gateway.factoryCancelIdea) {
      throw new FactoryGatewayError("internal_error", "Idea cancellation unavailable");
    }
    return this.gateway.factoryCancelIdea(workspace.path, ideaId, confirm);
  }
}

export class RevokeFactoryIdeaReadyUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, ideaId: string): Promise<Idea> {
    requireIdeaId(ideaId);
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryRevokeReady(workspace.path, ideaId);
  }
}
