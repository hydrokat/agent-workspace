export interface AgentQuery {
  id: string;
  task: string;
  question: string;
  options?: string[];
  status: "open" | "answered" | "withdrawn" | "acknowledged";
  created: number;
  answer?: string;
  answered_via?: string;
  answered_at?: number;
}

export function isValidQueryId(id: string): boolean {
  return /^q-[a-z0-9]{4,32}$/.test(id);
}
