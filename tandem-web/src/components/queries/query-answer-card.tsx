"use client";

import { useState } from "react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { QuickOptionChips, extractOptionsFromText } from "./quick-option-chips";
import { HelpCircle, CheckCircle2, Send } from "lucide-react";
import type { AgentQuery } from "@/domain/models/query";

interface QueryAnswerCardProps {
  query: AgentQuery;
  onAnswer: (queryId: string, answer: string) => Promise<void>;
}

export function QueryAnswerCard({ query, onAnswer }: QueryAnswerCardProps) {
  const [answerText, setAnswerText] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const isAnswered = query.status === "answered";

  const options =
    query.options && query.options.length > 0
      ? query.options
      : extractOptionsFromText(query.question);

  const handleSubmit = async (textToSubmit?: string) => {
    const finalAnswer = (textToSubmit || answerText).trim();
    if (!finalAnswer) return;

    try {
      setSubmitting(true);
      await onAnswer(query.id, finalAnswer);
      setAnswerText("");
    } finally {
      setSubmitting(false);
    }
  };

  const handleSelectOption = (option: string) => {
    setAnswerText(option);
  };

  return (
    <Card className="p-4 flex flex-col gap-3 transition-colors">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b pb-2">
        <div className="flex items-center gap-2">
          {isAnswered ? (
            <CheckCircle2 className="h-4 w-4 text-emerald-500" />
          ) : (
            <HelpCircle className="h-4 w-4 text-amber-500" />
          )}
          <span className="font-mono text-xs font-semibold text-foreground">
            {query.id}
          </span>
          <Badge
            variant={isAnswered ? "secondary" : "default"}
            className="text-[10px] capitalize"
          >
            {isAnswered ? "Answered" : "Open"}
          </Badge>
        </div>

        {query.task && (
          <span className="font-mono text-xs text-muted-foreground truncate">
            {query.task}
          </span>
        )}
      </div>

      <div className="text-sm text-foreground whitespace-pre-wrap break-words">
        {query.question}
      </div>

      {isAnswered ? (
        <div className="rounded-md border bg-muted/30 p-3 text-xs flex flex-col gap-1">
          <span className="font-semibold text-muted-foreground">Answer:</span>
          <p className="font-mono text-foreground whitespace-pre-wrap">{query.answer}</p>
        </div>
      ) : (
        <div className="flex flex-col gap-3 pt-1">
          {options.length > 0 && (
            <div className="flex flex-col gap-1.5">
              <span className="text-xs text-muted-foreground">Quick Options:</span>
              <QuickOptionChips
                options={options}
                onSelectOption={handleSelectOption}
                disabled={submitting}
              />
            </div>
          )}

          <div className="flex flex-col gap-2">
            <Textarea
              placeholder="Type your answer or select an option above…"
              value={answerText}
              onChange={(e) => setAnswerText(e.target.value)}
              disabled={submitting}
              rows={2}
              className="text-xs"
            />
            <Button
              type="button"
              className="min-h-11 gap-1.5 self-end text-xs"
              disabled={submitting || !answerText.trim()}
              onClick={() => handleSubmit()}
            >
              <Send className="h-3.5 w-3.5" />
              <span>Submit Answer</span>
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}
