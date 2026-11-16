import {
  isValidWorkspaceId,
  isValidLabel,
  normalizeLabel,
  LABEL_RULE_TEXT,
} from "../../domain/models/workspace";
import type { AllowedTandemAction, ITandemGateway } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

const ALLOWED_ACTIONS: Set<AllowedTandemAction> = new Set([
  "show",
  "select",
  "restart",
  "close-agent",
  "equalize",
  "reset-ui",
  "rename-workspace",
  "rename-agent",
]);

const PANE_OR_SESSION = /^(%[0-9]+|[a-z]+-[0-9]{1,2})$/;

function requireLabel(raw: string): string {
  const label = normalizeLabel(raw);
  if (!isValidLabel(label)) {
    throw new Error(`Invalid label: ${LABEL_RULE_TEXT}`);
  }
  return label;
}

function validateArgs(action: AllowedTandemAction, args: string[]): string[] {
  if (action === "rename-workspace") {
    if (args.length > 1) {
      throw new Error("Invalid workspace rename arguments");
    }
    return args.length === 0 ? [] : [requireLabel(args[0])];
  }

  if (action === "rename-agent") {
    if (args.length < 1 || args.length > 2 || !PANE_OR_SESSION.test(args[0])) {
      throw new Error("Invalid agent rename arguments");
    }
    return args.length === 1 ? [args[0]] : [args[0], requireLabel(args[1])];
  }

  if (action === "close-agent" && args.length > 0) {
    if (args.length !== 1 || !/^(%\d+|[a-z0-9-]+)$/.test(args[0])) {
      throw new Error(`Invalid close-agent target argument: '${args.join(" ")}'`);
    }
  }

  for (const arg of args) {
    if (!/^[a-zA-Z0-9%_.:][a-zA-Z0-9%_.:-]*$/.test(arg)) {
      throw new Error(`Argument contains invalid characters: '${arg}'`);
    }
  }

  return args;
}

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

    const validatedArgs = validateArgs(action, args);

    const workspace = await this.registry.findById(workspaceId);
    if (!workspace) {
      throw new Error(`Workspace not found: '${workspaceId}'`);
    }

    await this.gateway.executeAction(workspace.path, action, validatedArgs);
  }
}
