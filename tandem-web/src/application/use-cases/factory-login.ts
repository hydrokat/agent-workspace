import {
  FACTORY_CLIS,
  FactoryGatewayError,
  isValidCli,
  type AuthRiskAcknowledgement,
  type AuthRiskItem,
  type LoginPaneResult,
  type LoginStatusItem,
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

export class ListFactoryLoginStatusUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<LoginStatusItem[]> {
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryLoginStatus(workspace.path);
  }
}

export class OpenFactoryLoginUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, cli: string): Promise<LoginPaneResult> {
    if (!isValidCli(cli)) {
      throw new FactoryGatewayError(
        "invalid_input",
        `Unsupported CLI "${cli}" (expected ${FACTORY_CLIS.join(", ")})`
      );
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryLoginOpen(workspace.path, cli);
  }
}

const RISK_METHOD_PATTERN = /^[a-z][a-z0-9-]{0,31}$/;

export class ListFactoryAuthRiskUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<AuthRiskItem[]> {
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryLoginRisk(workspace.path);
  }
}

export class AcknowledgeFactoryAuthRiskUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    input: { cli: string; method: string; confirm: string }
  ): Promise<AuthRiskAcknowledgement> {
    if (!isValidCli(input.cli)) {
      throw new FactoryGatewayError(
        "invalid_input",
        `Unsupported CLI "${input.cli}" (expected ${FACTORY_CLIS.join(", ")})`
      );
    }
    if (!RISK_METHOD_PATTERN.test(input.method)) {
      throw new FactoryGatewayError("invalid_input", "Invalid sign-in method");
    }
    if (input.confirm !== input.cli) {
      throw new FactoryGatewayError(
        "invalid_input",
        `Type the CLI name (${input.cli}) to confirm`
      );
    }
    const workspace = await requireWorkspace(this.registry, workspaceId);
    return this.gateway.factoryAcknowledgeRisk(workspace.path, input.cli, input.method);
  }
}
