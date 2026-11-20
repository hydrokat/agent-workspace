import {
  FactoryGatewayError,
  isValidModelId,
  isValidWorkflowName,
  type FactoryBudget,
  type FactorySettings,
  type FactorySettingsChangeResult,
  type FactorySettingsPayload,
  type ProviderModelTiers,
} from "@/domain/models/factory";
import type { ITandemGateway } from "@/domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "@/domain/ports/workspace-registry";

const PROTECTED_KEYS = new Set([
  "provider_gates",
  "providers",
  "checks",
  "deploy",
  "deploy_command",
  "deploy_commands",
  "version",
  "allowed_tools",
  "denied_tools",
]);

const PLAIN_KEYS = new Set([
  "max_parallel_agents",
  "default_temperature",
  "default_workflow",
  "max_review_rounds",
  "approval_grace_secs",
  "default_worker_cli",
  "orchestrator_may_choose",
  "default_budget",
  "model_tiers",
]);

async function requireWorkspace(registry: IWorkspaceRegistry, workspaceId: string) {
  const workspace = await registry.findById(workspaceId);
  if (!workspace) {
    throw new FactoryGatewayError("not_found", "Workspace not found");
  }
  return workspace;
}

export interface PlainFactorySettingsUpdate {
  max_parallel_agents?: number;
  default_temperature?: number;
  default_workflow?: string;
  max_review_rounds?: number;
  approval_grace_secs?: number;
  default_worker_cli?: "auto" | "claude" | "agy" | "opencode";
  orchestrator_may_choose?: boolean;
  default_budget?: FactoryBudget;
  model_tiers?: Record<string, ProviderModelTiers>;
}

function validateBudget(budget: unknown): asserts budget is FactoryBudget {
  if (!budget || typeof budget !== "object" || Array.isArray(budget)) {
    throw new FactoryGatewayError("invalid_input", "default_budget must be an object");
  }
  const b = budget as Record<string, unknown>;
  if (b.max_cost_usd !== undefined && b.max_cost_usd !== null) {
    if (typeof b.max_cost_usd !== "number" || !Number.isFinite(b.max_cost_usd) || b.max_cost_usd < 0) {
      throw new FactoryGatewayError("invalid_input", "default_budget.max_cost_usd must be >= 0.0");
    }
  }
  if (b.max_duration_secs !== undefined && b.max_duration_secs !== null) {
    if (
      typeof b.max_duration_secs !== "number" ||
      !Number.isInteger(b.max_duration_secs) ||
      b.max_duration_secs < 0
    ) {
      throw new FactoryGatewayError(
        "invalid_input",
        "default_budget.max_duration_secs must be a non-negative integer"
      );
    }
  }
  if (b.max_retries !== undefined && b.max_retries !== null) {
    if (
      typeof b.max_retries !== "number" ||
      !Number.isInteger(b.max_retries) ||
      b.max_retries < 0
    ) {
      throw new FactoryGatewayError(
        "invalid_input",
        "default_budget.max_retries must be a non-negative integer"
      );
    }
  }
}

function validateModelTiers(tiers: unknown): asserts tiers is Record<string, ProviderModelTiers> {
  if (!tiers || typeof tiers !== "object" || Array.isArray(tiers)) {
    throw new FactoryGatewayError("invalid_input", "model_tiers must be an object");
  }
  for (const [provider, value] of Object.entries(tiers)) {
    if (!provider.trim()) {
      throw new FactoryGatewayError("invalid_input", "model_tiers provider name must not be empty");
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new FactoryGatewayError("invalid_input", `model_tiers.${provider} must be an object`);
    }
    const t = value as Record<string, unknown>;
    for (const tierName of ["economy", "standard", "premium"] as const) {
      const id = t[tierName];
      if (typeof id !== "string" || !isValidModelId(id)) {
        throw new FactoryGatewayError(
          "invalid_input",
          `model_tiers.${provider}.${tierName} must be a valid model id`
        );
      }
    }
  }
}

function validatePlainUpdate(payload: unknown): PlainFactorySettingsUpdate {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new FactoryGatewayError("invalid_input", "Settings payload must be a JSON object");
  }
  const obj = payload as Record<string, unknown>;
  for (const key of Object.keys(obj)) {
    if (PROTECTED_KEYS.has(key)) {
      throw new FactoryGatewayError(
        "forbidden",
        `Setting "${key}" cannot be changed via tandem-web; use tandem on the host`
      );
    }
    if (!PLAIN_KEYS.has(key)) {
      throw new FactoryGatewayError("invalid_input", `Unknown setting key "${key}"`);
    }
  }

  if (obj.max_parallel_agents !== undefined) {
    const v = obj.max_parallel_agents;
    if (typeof v !== "number" || !Number.isInteger(v) || v < 1 || v > 8) {
      throw new FactoryGatewayError(
        "invalid_input",
        "max_parallel_agents must be an integer between 1 and 8"
      );
    }
  }
  if (obj.default_temperature !== undefined) {
    const v = obj.default_temperature;
    if (typeof v !== "number" || !Number.isFinite(v) || v < 0 || v > 1) {
      throw new FactoryGatewayError(
        "invalid_input",
        "default_temperature must be a number between 0.0 and 1.0"
      );
    }
  }
  if (obj.max_review_rounds !== undefined) {
    const v = obj.max_review_rounds;
    if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 5) {
      throw new FactoryGatewayError(
        "invalid_input",
        "max_review_rounds must be an integer between 0 and 5"
      );
    }
  }
  if (obj.approval_grace_secs !== undefined) {
    const v = obj.approval_grace_secs;
    if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 600) {
      throw new FactoryGatewayError(
        "invalid_input",
        "approval_grace_secs must be an integer between 0 and 600"
      );
    }
  }
  if (obj.default_workflow !== undefined) {
    const v = obj.default_workflow;
    if (typeof v !== "string" || !isValidWorkflowName(v)) {
      throw new FactoryGatewayError(
        "invalid_input",
        "default_workflow must match ^[a-z][a-z0-9-]{0,47}$"
      );
    }
  }
  if (obj.default_worker_cli !== undefined) {
    const v = obj.default_worker_cli;
    if (v !== "auto" && v !== "claude" && v !== "agy" && v !== "opencode") {
      throw new FactoryGatewayError(
        "invalid_input",
        "default_worker_cli must be auto, claude, agy, or opencode"
      );
    }
  }
  if (obj.orchestrator_may_choose !== undefined && typeof obj.orchestrator_may_choose !== "boolean") {
    throw new FactoryGatewayError("invalid_input", "orchestrator_may_choose must be a boolean");
  }
  if (obj.default_budget !== undefined) {
    validateBudget(obj.default_budget);
  }
  if (obj.model_tiers !== undefined) {
    validateModelTiers(obj.model_tiers);
  }

  return obj as PlainFactorySettingsUpdate;
}

export class GetFactorySettingsUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string): Promise<FactorySettingsPayload> {
    const workspace = await requireWorkspace(this.registry, workspaceId);
    const [settings, providers, workflows] = await Promise.all([
      this.gateway.factoryGetSettings(workspace.path),
      this.gateway.factoryProviderStatus(workspace.path),
      this.gateway.factoryListWorkflows(workspace.path),
    ]);
    return {
      settings,
      providers,
      workflows: workflows.map((w) => ({
        name: w.name,
        version: w.version,
        valid: w.valid,
      })),
    };
  }
}

export class UpdateFactorySettingsUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(
    workspaceId: string,
    payload: unknown
  ): Promise<{ settings: FactorySettings; changes: FactorySettingsChangeResult[] }> {
    const updates = validatePlainUpdate(payload);
    const workspace = await requireWorkspace(this.registry, workspaceId);
    let current = await this.gateway.factoryGetSettings(workspace.path);
    const changes: FactorySettingsChangeResult[] = [];

    const pairs: Array<[string, string, boolean]> = [];
    if (
      updates.max_parallel_agents !== undefined &&
      updates.max_parallel_agents !== current.max_parallel_agents
    ) {
      pairs.push(["max_parallel_agents", String(updates.max_parallel_agents), true]);
    }
    if (
      updates.default_temperature !== undefined &&
      updates.default_temperature !== current.default_temperature
    ) {
      pairs.push(["default_temperature", String(updates.default_temperature), true]);
    }
    if (
      updates.max_review_rounds !== undefined &&
      updates.max_review_rounds !== current.max_review_rounds
    ) {
      pairs.push(["max_review_rounds", String(updates.max_review_rounds), true]);
    }
    if (
      updates.default_workflow !== undefined &&
      updates.default_workflow !== current.default_workflow
    ) {
      pairs.push(["default_workflow", updates.default_workflow, true]);
    }
    if (
      updates.approval_grace_secs !== undefined &&
      updates.approval_grace_secs !== current.approval_grace_secs
    ) {
      pairs.push(["approval_grace_secs", String(updates.approval_grace_secs), true]);
    }
    if (
      updates.default_worker_cli !== undefined &&
      updates.default_worker_cli !== (current.default_worker_cli ?? "auto")
    ) {
      pairs.push(["default_worker_cli", updates.default_worker_cli, true]);
    }
    if (
      updates.orchestrator_may_choose !== undefined &&
      updates.orchestrator_may_choose !== (current.orchestrator_may_choose ?? true)
    ) {
      pairs.push(["orchestrator_may_choose", String(updates.orchestrator_may_choose), true]);
    }
    if (
      updates.default_budget !== undefined &&
      JSON.stringify(updates.default_budget) !== JSON.stringify(current.default_budget)
    ) {
      pairs.push(["default_budget", JSON.stringify(updates.default_budget), true]);
    }
    if (
      updates.model_tiers !== undefined &&
      JSON.stringify(updates.model_tiers) !== JSON.stringify(current.model_tiers)
    ) {
      pairs.push(["model_tiers", JSON.stringify(updates.model_tiers), true]);
    }

    for (const [key, value] of pairs) {
      const res = await this.gateway.factorySetSetting(workspace.path, key, value);
      changes.push(res);
      current = res.settings;
    }

    return { settings: current, changes };
  }
}
