import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import ReactMarkdown from "react-markdown";
import {
  Mic,
  Square,
  Sparkles,
  Copy,
  Check,
  Trash2,
  ChevronDown,
  ChevronRight,
  Settings2,
  Loader2,
} from "lucide-react";
import {
  commands,
  events,
  type Meeting,
  type MeetingUpdatePayload,
} from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "../../ui/Button";

// SecretMap key the backend stores the Kantega LLM proxy key under
// (mirror of crate::kantega_llm::MEETING_LLM_KEY_ID).
const LLM_KEY_ID = "kantega_llmproxy";

type RecordPhase = "idle" | "recording" | "transcribing";

function formatDuration(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

export const MeetingSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, refreshSettings } = useSettings();

  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [phase, setPhase] = useState<RecordPhase>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [summarizingId, setSummarizingId] = useState<number | null>(null);
  const [showSettings, setShowSettings] = useState(false);

  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const model = settings?.meeting_summary_model ?? "";
  const hasKey = Boolean(settings?.meeting_llm_api_keys?.[LLM_KEY_ID]);

  // Load existing meetings + subscribe to live updates.
  useEffect(() => {
    commands.getMeetings().then((r) => {
      if (r.status === "ok") setMeetings(r.data);
    });
    const unlisten = events.meetingUpdatePayload.listen((event) => {
      const p: MeetingUpdatePayload = event.payload;
      if (p.action === "added") {
        setMeetings((prev) => [p.meeting, ...prev]);
      } else if (p.action === "updated") {
        setMeetings((prev) =>
          prev.map((m) => (m.id === p.meeting.id ? p.meeting : m)),
        );
      } else if (p.action === "deleted") {
        setMeetings((prev) => prev.filter((m) => m.id !== p.id));
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Recording timer.
  useEffect(() => {
    if (phase === "recording") {
      timerRef.current = setInterval(() => setElapsed((e) => e + 1), 1000);
    } else if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [phase]);

  const startRecording = async () => {
    const r = await commands.startMeetingRecording();
    if (r.status !== "ok") {
      toast.error(t("meeting.errors.startFailed"), {
        description: String(r.error),
      });
      return;
    }
    setElapsed(0);
    setPhase("recording");
  };

  // Auto-summarise a freshly transcribed meeting when a key is present.
  const summarize = useCallback(
    async (id: number) => {
      setSummarizingId(id);
      try {
        const r = await commands.summarizeMeeting(id);
        if (r.status !== "ok") {
          toast.error(t("meeting.errors.summaryFailed"), {
            description: String(r.error),
          });
        }
      } finally {
        setSummarizingId(null);
      }
    },
    [t],
  );

  const stopRecording = async () => {
    setPhase("transcribing");
    try {
      const r = await commands.stopMeetingRecording();
      if (r.status !== "ok") {
        toast.error(t("meeting.errors.transcribeFailed"), {
          description: String(r.error),
        });
        return;
      }
      // Meeting is added via the event listener. Auto-summarise if we have a key.
      if (hasKey) {
        summarize(r.data.id);
      }
    } finally {
      setPhase("idle");
    }
  };

  const deleteMeeting = async (id: number) => {
    setMeetings((prev) => prev.filter((m) => m.id !== id));
    const r = await commands.deleteMeeting(id);
    if (r.status !== "ok") {
      toast.error(t("meeting.errors.deleteFailed"));
      commands.getMeetings().then((res) => {
        if (res.status === "ok") setMeetings(res.data);
      });
    }
  };

  const saveApiKey = async (key: string) => {
    const r = await commands.changeMeetingLlmApiKeySetting(key);
    if (r.status !== "ok") {
      toast.error(t("meeting.errors.saveFailed"));
      return;
    }
    await refreshSettings();
    toast.success(t("meeting.settings.saved"));
  };

  const isBusy = phase !== "idle";

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {/* Recorder */}
      <div className="space-y-2">
        <div className="px-4 flex items-center justify-between">
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide">
            {t("meeting.title")}
          </h2>
          <button
            onClick={() => setShowSettings((s) => !s)}
            className="flex items-center gap-1.5 text-sm text-text/60 hover:text-logo-primary transition-colors cursor-pointer"
            title={t("meeting.settings.title")}
          >
            <Settings2 className="w-4 h-4" />
            {t("meeting.settings.title")}
          </button>
        </div>

        <div className="bg-background border border-mid-gray/20 rounded-lg p-4 flex flex-col items-center gap-3">
          {phase === "recording" ? (
            <>
              <div className="flex items-center gap-2 text-logo-primary">
                <span className="w-2.5 h-2.5 rounded-full bg-logo-primary animate-pulse" />
                <span className="font-mono text-lg">
                  {formatDuration(elapsed)}
                </span>
              </div>
              <Button
                onClick={stopRecording}
                variant="primary"
                size="md"
                className="flex items-center gap-2"
              >
                <Square className="w-4 h-4" />
                {t("meeting.stop")}
              </Button>
            </>
          ) : phase === "transcribing" ? (
            <div className="flex items-center gap-2 text-text/70 py-2">
              <Loader2 className="w-4 h-4 animate-spin" />
              {t("meeting.transcribing")}
            </div>
          ) : (
            <Button
              onClick={startRecording}
              variant="primary"
              size="md"
              className="flex items-center gap-2"
              disabled={isBusy}
            >
              <Mic className="w-4 h-4" />
              {t("meeting.record")}
            </Button>
          )}
          <p className="text-xs text-text/50 text-center max-w-md">
            {t("meeting.captureHint")}
          </p>
        </div>

        {!hasKey && (
          <div className="mx-4 text-xs text-warning bg-warning/10 border border-warning/20 rounded-md px-3 py-2">
            {t("meeting.noKey")}
          </div>
        )}
      </div>

      {/* Summary settings (collapsible) */}
      {showSettings && (
        <MeetingSettingsPanel
          model={model}
          hasKey={hasKey}
          prompt={settings?.meeting_summary_prompt ?? ""}
          onSaveKey={saveApiKey}
          onSaveModel={(m) => updateSetting("meeting_summary_model", m)}
          onSavePrompt={(p) => updateSetting("meeting_summary_prompt", p)}
        />
      )}

      {/* Meetings list */}
      <div className="space-y-2">
        <h2 className="px-4 text-xs font-medium text-mid-gray uppercase tracking-wide">
          {t("meeting.pastMeetings")}
        </h2>
        <div className="bg-background border border-mid-gray/20 rounded-lg">
          {meetings.length === 0 ? (
            <div className="px-4 py-6 text-center text-text/60 text-sm">
              {t("meeting.empty")}
            </div>
          ) : (
            <div className="divide-y divide-mid-gray/20">
              {meetings.map((m) => (
                <MeetingRow
                  key={m.id}
                  meeting={m}
                  model={model}
                  hasKey={hasKey}
                  summarizing={summarizingId === m.id}
                  onSummarize={() => summarize(m.id)}
                  onDelete={() => deleteMeeting(m.id)}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

interface PanelProps {
  model: string;
  hasKey: boolean;
  prompt: string;
  onSaveKey: (key: string) => void;
  onSaveModel: (model: string) => void;
  onSavePrompt: (prompt: string) => void;
}

const MeetingSettingsPanel: React.FC<PanelProps> = ({
  model,
  hasKey,
  prompt,
  onSaveKey,
  onSaveModel,
  onSavePrompt,
}) => {
  const { t } = useTranslation();
  const [keyInput, setKeyInput] = useState("");
  const [modelInput, setModelInput] = useState(model);
  const [promptInput, setPromptInput] = useState(prompt);

  useEffect(() => setModelInput(model), [model]);
  useEffect(() => setPromptInput(prompt), [prompt]);

  return (
    <div className="mx-4 bg-background border border-mid-gray/20 rounded-lg p-4 space-y-4">
      {/* API key */}
      <div className="space-y-1.5">
        <label className="text-sm font-medium">
          {t("meeting.settings.apiKey")}
        </label>
        <div className="flex gap-2">
          <input
            type="password"
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            placeholder={
              hasKey
                ? t("meeting.settings.apiKeySet")
                : t("meeting.settings.apiKeyPlaceholder")
            }
            className="flex-1 px-3 py-1.5 text-sm rounded-md bg-background border border-mid-gray/30 focus:border-logo-primary outline-none"
          />
          <Button
            onClick={() => {
              if (keyInput.trim()) {
                onSaveKey(keyInput.trim());
                setKeyInput("");
              }
            }}
            variant="secondary"
            size="sm"
            disabled={!keyInput.trim()}
          >
            {t("meeting.settings.save")}
          </Button>
        </div>
        <p className="text-xs text-text/50">{t("meeting.settings.apiKeyHelp")}</p>
      </div>

      {/* Model */}
      <div className="space-y-1.5">
        <label className="text-sm font-medium">
          {t("meeting.settings.model")}
        </label>
        <div className="flex gap-2">
          <input
            type="text"
            value={modelInput}
            onChange={(e) => setModelInput(e.target.value)}
            className="flex-1 px-3 py-1.5 text-sm rounded-md bg-background border border-mid-gray/30 focus:border-logo-primary outline-none font-mono"
          />
          <Button
            onClick={() => onSaveModel(modelInput.trim())}
            variant="secondary"
            size="sm"
            disabled={modelInput.trim() === model}
          >
            {t("meeting.settings.save")}
          </Button>
        </div>
      </div>

      {/* Prompt */}
      <div className="space-y-1.5">
        <label className="text-sm font-medium">
          {t("meeting.settings.prompt")}
        </label>
        <textarea
          value={promptInput}
          onChange={(e) => setPromptInput(e.target.value)}
          rows={8}
          className="w-full px-3 py-2 text-sm rounded-md bg-background border border-mid-gray/30 focus:border-logo-primary outline-none resize-y whitespace-pre-wrap"
        />
        <div className="flex justify-end">
          <Button
            onClick={() => onSavePrompt(promptInput)}
            variant="secondary"
            size="sm"
            disabled={promptInput === prompt}
          >
            {t("meeting.settings.savePrompt")}
          </Button>
        </div>
      </div>
    </div>
  );
};

interface RowProps {
  meeting: Meeting;
  model: string;
  hasKey: boolean;
  summarizing: boolean;
  onSummarize: () => void;
  onDelete: () => void;
}

const MeetingRow: React.FC<RowProps> = ({
  meeting,
  model,
  hasKey,
  summarizing,
  onSummarize,
  onDelete,
}) => {
  const { t } = useTranslation();
  const [showTranscript, setShowTranscript] = useState(false);
  const [copied, setCopied] = useState<"summary" | "transcript" | null>(null);

  const copy = async (text: string, which: "summary" | "transcript") => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      setTimeout(() => setCopied(null), 2000);
    } catch {
      /* ignore */
    }
  };

  const date = new Date(meeting.timestamp * 1000).toLocaleString();

  return (
    <div className="px-4 py-3 flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm font-medium">{meeting.title}</p>
          <p className="text-xs text-text/50">
            {date} · {formatDuration(meeting.duration_secs)}
          </p>
        </div>
        <div className="flex items-center gap-1">
          <Button
            onClick={onSummarize}
            variant="secondary"
            size="sm"
            className="flex items-center gap-1.5"
            disabled={summarizing || !hasKey}
            title={hasKey ? "" : t("meeting.addKeyHint")}
          >
            {summarizing ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Sparkles className="w-3.5 h-3.5" />
            )}
            {meeting.summary
              ? t("meeting.regenerate")
              : t("meeting.generateSummary")}
          </Button>
          <button
            onClick={onDelete}
            className="p-1.5 rounded-md text-text/50 hover:text-logo-primary transition-colors cursor-pointer"
            title={t("meeting.delete")}
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Summary */}
      {meeting.summary ? (
        <div className="rounded-md bg-mid-gray/5 border border-mid-gray/15 p-3 space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-[10px] uppercase tracking-wide font-medium text-logo-primary/90 bg-logo-primary/10 px-1.5 py-0.5 rounded">
              {t("meeting.aiBadge", { model: meeting.model ?? model })}
            </span>
            <button
              onClick={() => copy(meeting.summary ?? "", "summary")}
              className="p-1 rounded text-text/50 hover:text-logo-primary transition-colors cursor-pointer"
              title={t("meeting.copySummary")}
            >
              {copied === "summary" ? (
                <Check className="w-3.5 h-3.5" />
              ) : (
                <Copy className="w-3.5 h-3.5" />
              )}
            </button>
          </div>
          <div className="prose-sm max-w-none text-sm text-text/90 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_strong]:font-semibold [&_h1]:font-semibold [&_h2]:font-semibold [&_h3]:font-semibold space-y-1 whitespace-normal">
            <ReactMarkdown>{meeting.summary}</ReactMarkdown>
          </div>
          <p className="text-[10px] text-text/40 italic">
            {t("meeting.disclaimer")}
          </p>
        </div>
      ) : summarizing ? (
        <div className="flex items-center gap-2 text-sm text-text/60">
          <Loader2 className="w-4 h-4 animate-spin" />
          {t("meeting.summarizing")}
        </div>
      ) : null}

      {/* Transcript (collapsible) */}
      <div>
        <div className="flex items-center justify-between">
          <button
            onClick={() => setShowTranscript((s) => !s)}
            className="flex items-center gap-1 text-xs text-text/60 hover:text-logo-primary transition-colors cursor-pointer"
          >
            {showTranscript ? (
              <ChevronDown className="w-3.5 h-3.5" />
            ) : (
              <ChevronRight className="w-3.5 h-3.5" />
            )}
            {t("meeting.transcript")}
          </button>
          {showTranscript && meeting.transcript && (
            <button
              onClick={() => copy(meeting.transcript, "transcript")}
              className="p-1 rounded text-text/50 hover:text-logo-primary transition-colors cursor-pointer"
              title={t("meeting.copyTranscript")}
            >
              {copied === "transcript" ? (
                <Check className="w-3.5 h-3.5" />
              ) : (
                <Copy className="w-3.5 h-3.5" />
              )}
            </button>
          )}
        </div>
        {showTranscript && (
          <p className="mt-2 text-sm text-text/80 whitespace-pre-wrap break-words select-text">
            {meeting.transcript || t("meeting.noTranscript")}
          </p>
        )}
      </div>
    </div>
  );
};
