import type {
  ClaimInfo,
  OngoingTaskInfo,
  PaneInfo,
  PhaseInfo,
  SessionInfo,
  SprintView,
} from "@/domain/models/snapshot";

export type TaskState = "done" | "in_progress" | "blocked" | "pending";

export interface AgentRef {
  session: string;
  label: string;
  paneId: string | null;
  dead: boolean;
  stale: boolean;
}

export interface DrilldownTask {
  id: string;
  title: string;
  status: string;
  state: TaskState;
  objectivesDone: number;
  objectivesTotal: number;
  agent: AgentRef | null;
  statusMismatch: boolean;
}

export interface DrilldownPhase {
  name: string;
  title: string;
  moscow: string;
  effort: number;
  done: number;
  total: number;
  archived: boolean;
  tasks: DrilldownTask[];
  tasksAvailable: boolean;
}

export function mapTaskState(status: string): TaskState {
  const lower = status.toLowerCase();
  if (lower.includes("done") || lower.includes("complete")) {
    return "done";
  }
  if (lower.includes("progress")) {
    return "in_progress";
  }
  if (lower.includes("block")) {
    return "blocked";
  }
  return "pending";
}

export function buildSprintDrilldown(input: {
  sprint: SprintView;
  phases: PhaseInfo[];
  ongoingTasks: OngoingTaskInfo[];
  claims: ClaimInfo[];
  sessions: SessionInfo[];
  panes: PaneInfo[];
}): DrilldownPhase[] {
  const sprintPhases = input.sprint?.phases ?? [];
  if (sprintPhases.length === 0) {
    return [];
  }

  const phaseByName = new Map<string, PhaseInfo>();
  for (const phase of input.phases) {
    phaseByName.set(phase.name, phase);
  }

  const ongoingByKey = new Map<string, OngoingTaskInfo>();
  for (const ot of input.ongoingTasks) {
    ongoingByKey.set(`${ot.phase}::${ot.task_id}`, ot);
  }

  const claimByKey = new Map<string, ClaimInfo>();
  for (const claim of input.claims) {
    claimByKey.set(`${claim.phase}::${claim.task}`, claim);
  }

  const sessionById = new Map<string, SessionInfo>();
  for (const sess of input.sessions) {
    sessionById.set(sess.id, sess);
  }

  const paneBySession = new Map<string, PaneInfo>();
  for (const pane of input.panes) {
    if (pane.session) {
      paneBySession.set(pane.session, pane);
    }
  }

  return sprintPhases.map((sp) => {
    const phaseInfo = phaseByName.get(sp.name);
    if (!phaseInfo) {
      return {
        name: sp.name,
        title: "",
        moscow: sp.moscow,
        effort: sp.effort,
        done: sp.done,
        total: sp.total,
        archived: Boolean(sp.archived),
        tasks: [],
        tasksAvailable: false,
      };
    }

    const tasks: DrilldownTask[] = (phaseInfo.tasks ?? []).map((task) => {
      const key = `${sp.name}::${task.id}`;
      const ongoing = ongoingByKey.get(key);
      const claim = ongoing?.claim ?? claimByKey.get(key) ?? null;

      let agent: AgentRef | null = null;
      if (claim) {
        const pane = paneBySession.get(claim.session);
        const sess = sessionById.get(claim.session);
        const label =
          pane?.name?.trim() || sess?.name?.trim() || claim.session;
        agent = {
          session: claim.session,
          label,
          paneId: pane ? pane.id : null,
          dead: pane ? Boolean(pane.dead) : true,
          stale: Boolean(ongoing?.stale),
        };
      }

      return {
        id: task.id,
        title: task.title,
        status: task.status,
        state: mapTaskState(task.status),
        objectivesDone: task.objectives_done ?? 0,
        objectivesTotal: task.objectives_total ?? 0,
        agent,
        statusMismatch: Boolean(ongoing?.status_mismatch),
      };
    });

    return {
      name: sp.name,
      title: phaseInfo.title,
      moscow: sp.moscow,
      effort: sp.effort,
      done: sp.done,
      total: sp.total,
      archived: Boolean(sp.archived ?? phaseInfo.archived),
      tasks,
      tasksAvailable: true,
    };
  });
}
