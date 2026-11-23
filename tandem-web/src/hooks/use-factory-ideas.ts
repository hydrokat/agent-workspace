"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  BoardMove,
  FactoryCli,
  ModelOption,
  Idea,
  IdeaCard,
  IdeaDetail,
  ReadyResult,
} from "@/domain/models/factory";

interface UseFactoryIdeasOptions {
  workspaceId: string | null;
  streamIdeas?: IdeaCard[];
}

export function useFactoryIdeas({ workspaceId, streamIdeas }: UseFactoryIdeasOptions) {
  const [selectedIdeaId, setSelectedIdeaId] = useState<string | null>(null);
  const [detail, setDetail] = useState<IdeaDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [planMarkdown, setPlanMarkdown] = useState<string | null>(null);
  const [feasibilityMarkdown, setFeasibilityMarkdown] = useState<string | null>(null);
  const [artifactsLoading, setArtifactsLoading] = useState(false);

  const requestCounterRef = useRef(0);

  useEffect(() => {
    setSelectedIdeaId(null);
    setDetail(null);
    setDetailError(null);
    setPlanMarkdown(null);
    setFeasibilityMarkdown(null);
  }, [workspaceId]);

  const streamEntryKey = useMemo(() => {
    if (!selectedIdeaId || !streamIdeas) return "";
    const found = streamIdeas.find((item) => item.id === selectedIdeaId);
    if (!found) return "";
    return `${found.id}:${found.state}:${found.updated}:${found.blockers}`;
  }, [selectedIdeaId, streamIdeas]);

  const fetchDetailAndArtifacts = useCallback(
    async (ideaId: string) => {
      if (!workspaceId) return;
      const myCounter = ++requestCounterRef.current;
      setDetailLoading(true);
      setDetailError(null);

      try {
        const res = await fetch(`/api/workspaces/${workspaceId}/factory/ideas/${ideaId}`, {
          cache: "no-store",
        });
        if (myCounter < requestCounterRef.current) return;

        if (!res.ok) {
          const body = await res.json().catch(() => null);
          const message = body?.error?.message || `Failed to load idea (${res.status})`;
          setDetailError(message);
          setDetailLoading(false);
          return;
        }

        const loadedDetail = (await res.json()) as IdeaDetail;
        if (myCounter < requestCounterRef.current) return;
        setDetail(loadedDetail);
        setDetailLoading(false);

        const planVersion = loadedDetail.idea.plan?.artifact.version;
        const feasibilityVersion = loadedDetail.idea.feasibility?.version;

        if (!planVersion && !feasibilityVersion) {
          setPlanMarkdown(null);
          setFeasibilityMarkdown(null);
          return;
        }

        setArtifactsLoading(true);
        const [planText, feasibilityText] = await Promise.all([
          planVersion
            ? fetch(
                `/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/artifacts/plan?version=${planVersion}`,
                { cache: "no-store" }
              ).then((r) => (r.ok ? r.text() : null))
            : Promise.resolve(null),
          feasibilityVersion
            ? fetch(
                `/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/artifacts/feasibility?version=${feasibilityVersion}`,
                { cache: "no-store" }
              ).then((r) => (r.ok ? r.text() : null))
            : Promise.resolve(null),
        ]);

        if (myCounter < requestCounterRef.current) return;
        setPlanMarkdown(planText);
        setFeasibilityMarkdown(feasibilityText);
      } catch (err: unknown) {
        if (myCounter < requestCounterRef.current) return;
        setDetailError(err instanceof Error ? err.message : "Failed to load idea");
      } finally {
        if (myCounter === requestCounterRef.current) {
          setDetailLoading(false);
          setArtifactsLoading(false);
        }
      }
    },
    [workspaceId]
  );

  useEffect(() => {
    if (!selectedIdeaId || !workspaceId) {
      setDetail(null);
      setDetailError(null);
      setPlanMarkdown(null);
      setFeasibilityMarkdown(null);
      return;
    }
    fetchDetailAndArtifacts(selectedIdeaId);
  }, [selectedIdeaId, workspaceId, streamEntryKey, fetchDetailAndArtifacts]);

  const refreshDetail = useCallback(() => {
    if (selectedIdeaId) {
      fetchDetailAndArtifacts(selectedIdeaId);
    }
  }, [selectedIdeaId, fetchDetailAndArtifacts]);

  const startIdea = useCallback(
    async (input: {
      cli: FactoryCli;
      model: string;
      title?: string;
      text?: string;
    }): Promise<Idea> => {
      if (!workspaceId) {
        throw new Error("No active workspace selected.");
      }
      const res = await fetch(`/api/workspaces/${workspaceId}/factory/ideas`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(input),
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        throw new Error(body?.error?.message || `Failed to start idea (${res.status})`);
      }
      const created = body.idea as Idea;
      setSelectedIdeaId(created.id);
      return created;
    },
    [workspaceId]
  );

  const listModels = useCallback(
    async (cli: FactoryCli): Promise<ModelOption[]> => {
      if (!workspaceId) {
        throw new Error("No active workspace selected.");
      }
      const res = await fetch(
        `/api/workspaces/${workspaceId}/factory/models?cli=${encodeURIComponent(cli)}`,
        { cache: "no-store" }
      );
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        throw new Error(body?.error?.message || `Failed to list models (${res.status})`);
      }
      return (body?.models ?? []) as ModelOption[];
    },
    [workspaceId]
  );

  const markReady = useCallback(
    async (
      ideaId: string,
      confirm: string,
      options?: { workflow?: string; deploy?: boolean; target?: string }
    ): Promise<ReadyResult> => {
      if (!workspaceId) {
        throw new Error("No active workspace selected.");
      }
      const res = await fetch(`/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/ready`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ confirm, ...options }),
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        throw new Error(body?.error?.message || `Unable to mark idea ready (${res.status})`);
      }
      await fetchDetailAndArtifacts(ideaId);
      return body as ReadyResult;
    },
    [workspaceId, fetchDetailAndArtifacts]
  );

  const revokeReady = useCallback(
    async (ideaId: string): Promise<Idea> => {
      if (!workspaceId) {
        throw new Error("No active workspace selected.");
      }
      const res = await fetch(`/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/ready`, {
        method: "DELETE",
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        throw new Error(body?.error?.message || `Unable to revoke ready state (${res.status})`);
      }
      await fetchDetailAndArtifacts(ideaId);
      return body.idea as Idea;
    },
    [workspaceId, fetchDetailAndArtifacts]
  );

  const cancelIdea = useCallback(
    async (ideaId: string): Promise<void> => {
      if (!workspaceId) {
        throw new Error("No active workspace selected.");
      }
      const res = await fetch(`/api/workspaces/${workspaceId}/factory/ideas/${ideaId}/cancel`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ confirm: ideaId }),
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        throw new Error(body?.error?.message || `Unable to cancel idea (${res.status})`);
      }
    },
    [workspaceId]
  );

  const moveIdea = useCallback(
    async (idea: IdeaCard, move: BoardMove): Promise<void> => {
      if (move.via === "revoke_ready") {
        await revokeReady(idea.id);
      } else if (move.via === "cancel") {
        await cancelIdea(idea.id);
      } else {
        throw new Error("Marking an idea ready needs the Gate 2 confirmation dialog.");
      }
    },
    [revokeReady, cancelIdea]
  );

  return {
    selectedIdeaId,
    selectIdea: setSelectedIdeaId,
    detail,
    detailLoading,
    detailError,
    planMarkdown,
    feasibilityMarkdown,
    artifactsLoading,
    refreshDetail,
    startIdea,
    listModels,
    markReady,
    revokeReady,
    cancelIdea,
    moveIdea,
  };
}
