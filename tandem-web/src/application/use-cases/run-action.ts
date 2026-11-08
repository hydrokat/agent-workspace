import { isValidWorkspaceId } from "../../domain/models/workspace";
import type { AllowedTandemAction, ITandemGateway } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

const ALLOWED_ACTIONS: Set<AllowedTandemAction> = new Set([
  "show",
  "select",
  "restart",
  "close-agent",
  "equalize",
  "reset-ui",
]);

export class RunWorkspaceActionUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    action: AllowedTandemAction,
    args: string[] = []
  ): Promise<void> {
    if (!isValidWorkspaceId(workspaceId)) {
      throw new Error(`Invalid workspace ID format: '${workspaceId}'`);
    }

    if (!ALLOWED_ACTIONS.has(action)) {
      throw new Error(`Forbidden action: '${action}'`);
    }

    for (const arg of args) {
      if (!/^[a-zA-Z0-9%_.:][a-zA-Z0-9%_.:-]*$/.test(arg)) {
        throw new Error(`Argument contains invalid characters: '${arg}'`);
      }
    }

    const workspace = await this.registry.findById(workspaceId);
    if (!workspace) {
      throw new Error(`Workspace not found: '${workspaceId}'`);
    }

    await this.gateway.executeAction(workspace.path, action, args);
  }
}
