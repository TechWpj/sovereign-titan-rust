import { useState, useEffect, useRef } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { User, Bot, Brain, Wrench, CheckCircle, XCircle, ChevronDown, ChevronRight, Layers, Sparkles, Volume2, VolumeX } from 'lucide-react';

function ThinkingBlock({ thinking, isStreaming }) {
  const [expanded, setExpanded] = useState(false);
  const wasStreaming = useRef(isStreaming);

  // Auto-collapse when streaming finishes
  useEffect(() => {
    if (wasStreaming.current && !isStreaming) {
      setExpanded(false);
    }
    wasStreaming.current = isStreaming;
  }, [isStreaming]);

  // Auto-expand while streaming
  useEffect(() => {
    if (isStreaming && thinking) {
      setExpanded(true);
    }
  }, [isStreaming, thinking]);

  if (!thinking) return null;

  return (
    <div className="mb-3 rounded-lg border border-purple-500/30 bg-purple-500/5 text-xs">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-1.5 px-3 py-2 text-purple-300 hover:text-purple-200 transition-colors"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Brain size={12} />
        <span className="font-medium">Thinking</span>
        {isStreaming && (
          <span className="ml-auto inline-block h-2 w-2 animate-pulse rounded-full bg-purple-400" />
        )}
      </button>

      {expanded && (
        <div className="px-3 pb-3">
          <pre className="max-h-80 overflow-auto rounded bg-titan-bg/80 p-3 text-titan-text-muted font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-words">
            {thinking}
            {isStreaming && (
              <span className="inline-block h-3 w-1 animate-pulse bg-purple-400 align-middle" />
            )}
          </pre>
        </div>
      )}
    </div>
  );
}

function PhaseActivity({ phases, isStreaming }) {
  const [expanded, setExpanded] = useState(true);
  const wasStreaming = useRef(isStreaming);

  useEffect(() => {
    if (wasStreaming.current && !isStreaming) {
      setExpanded(false);
    }
    wasStreaming.current = isStreaming;
  }, [isStreaming]);

  if (!phases || phases.length === 0) return null;

  return (
    <div className="mb-2 rounded-lg border border-amber-500/30 bg-amber-500/5 text-xs">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-1.5 px-3 py-1.5 text-amber-300 hover:text-amber-200 transition-colors"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Layers size={12} />
        <span>Deep Analysis ({phases.length} event{phases.length !== 1 ? 's' : ''})</span>
        {isStreaming && (
          <span className="ml-auto inline-block h-2 w-2 animate-pulse rounded-full bg-amber-400" />
        )}
      </button>

      {expanded && (
        <div className="flex flex-col gap-1 px-3 pb-2">
          {phases.map((phase, i) => (
            <PhaseStep key={i} phase={phase} />
          ))}
        </div>
      )}
    </div>
  );
}

function PhaseStep({ phase }) {
  const [detailOpen, setDetailOpen] = useState(false);

  if (phase.type === 'phase_start') {
    return (
      <div className="flex items-center gap-1.5 rounded bg-blue-500/10 px-2 py-1 border-l-2 border-blue-400">
        <span className="rounded-full bg-blue-500/20 px-1.5 py-0.5 text-blue-300 font-mono font-bold text-[10px]">
          P{phase.phase}
        </span>
        <span className="font-medium text-blue-300">{phase.name}</span>
        <span className="text-titan-text-muted truncate max-w-[200px]">{phase.description}</span>
      </div>
    );
  }

  if (phase.type === 'phase_complete') {
    return (
      <div>
        <button
          onClick={() => setDetailOpen((v) => !v)}
          className="flex items-center gap-1.5 text-titan-text-muted hover:text-titan-text transition-colors"
        >
          <CheckCircle size={12} className="shrink-0 text-green-400" />
          <span className="font-mono text-green-300">Phase {phase.phase}</span>
          <span className="truncate max-w-[250px]">
            {String(phase.findings || '').slice(0, 80)}
            {String(phase.findings || '').length > 80 ? '...' : ''}
          </span>
          {detailOpen ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        </button>
        {detailOpen && (
          <pre className="mt-1 ml-5 max-h-48 overflow-auto rounded bg-titan-bg p-2 text-titan-text-muted font-mono text-[10px] leading-tight whitespace-pre-wrap break-words">
            {phase.findings}
          </pre>
        )}
      </div>
    );
  }

  if (phase.type === 'synthesis_start') {
    return (
      <div className="flex items-center gap-1.5 rounded bg-amber-500/10 px-2 py-1 border-l-2 border-amber-400">
        <Sparkles size={12} className="text-amber-400 animate-pulse" />
        <span className="font-medium text-amber-300">Synthesizing findings...</span>
      </div>
    );
  }

  return null;
}

function ToolActivity({ toolCalls, isStreaming }) {
  const [expanded, setExpanded] = useState(true);
  const wasStreaming = useRef(isStreaming);

  // Auto-collapse when streaming finishes
  useEffect(() => {
    if (wasStreaming.current && !isStreaming) {
      setExpanded(false);
    }
    wasStreaming.current = isStreaming;
  }, [isStreaming]);

  if (!toolCalls || toolCalls.length === 0) return null;

  return (
    <div className="mb-2 rounded-lg border border-titan-border bg-titan-bg/50 text-xs">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-1.5 px-3 py-1.5 text-titan-text-muted hover:text-titan-text transition-colors"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Wrench size={12} />
        <span>Tool Activity ({toolCalls.length} step{toolCalls.length !== 1 ? 's' : ''})</span>
        {isStreaming && (
          <span className="ml-auto inline-block h-2 w-2 animate-pulse rounded-full bg-titan-accent" />
        )}
      </button>

      {expanded && (
        <div className="flex flex-col gap-1 px-3 pb-2">
          {toolCalls.map((tc, i) => (
            <ToolStep key={i} step={tc} />
          ))}
        </div>
      )}
    </div>
  );
}

function ToolStep({ step }) {
  const [detailOpen, setDetailOpen] = useState(false);

  if (step.type === 'thought') {
    return (
      <div className="flex items-start gap-1.5 text-titan-text-muted italic">
        <Brain size={12} className="mt-0.5 shrink-0 text-purple-400" />
        <span>{step.content}</span>
      </div>
    );
  }

  if (step.type === 'tool_call') {
    return (
      <div>
        <button
          onClick={() => setDetailOpen((v) => !v)}
          className="flex items-center gap-1.5 text-titan-text hover:text-titan-accent transition-colors"
        >
          <Wrench size={12} className="shrink-0 text-blue-400" />
          <span className="rounded-full bg-blue-500/20 px-2 py-0.5 text-blue-300 font-mono">
            {step.tool}
          </span>
          {step.input && Object.keys(step.input).length > 0 && (
            detailOpen ? <ChevronDown size={10} /> : <ChevronRight size={10} />
          )}
        </button>
        {detailOpen && step.input && (
          <pre className="mt-1 ml-5 max-h-32 overflow-auto rounded bg-titan-bg p-2 text-titan-text-muted font-mono text-[10px] leading-tight">
            {JSON.stringify(step.input, null, 2)}
          </pre>
        )}
      </div>
    );
  }

  if (step.type === 'tool_result') {
    const success = step.success !== false;
    return (
      <div>
        <button
          onClick={() => setDetailOpen((v) => !v)}
          className="flex items-center gap-1.5 text-titan-text-muted hover:text-titan-text transition-colors"
        >
          {success ? (
            <CheckCircle size={12} className="shrink-0 text-green-400" />
          ) : (
            <XCircle size={12} className="shrink-0 text-red-400" />
          )}
          <span className="font-mono">{step.tool}</span>
          <span className="truncate max-w-[300px]">
            {String(step.output || '').slice(0, 80)}
            {String(step.output || '').length > 80 ? '...' : ''}
          </span>
          {detailOpen ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        </button>
        {detailOpen && (
          <pre className="mt-1 ml-5 max-h-48 overflow-auto rounded bg-titan-bg p-2 text-titan-text-muted font-mono text-[10px] leading-tight whitespace-pre-wrap break-words">
            {step.output}
          </pre>
        )}
      </div>
    );
  }

  return null;
}

export default function MessageBubble({ message, isStreaming, onSpeak, isSpeaking, onStopSpeaking }) {
  const isUser = message.role === 'user';

  return (
    <div className={`flex gap-3 ${isUser ? 'flex-row-reverse' : ''}`}>
      <div
        className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg ${
          isUser ? 'bg-titan-accent' : 'bg-titan-surface'
        }`}
      >
        {isUser ? <User size={16} /> : <Bot size={16} />}
      </div>

      <div
        className={`max-w-[75%] rounded-2xl px-4 py-3 text-sm leading-relaxed ${
          isUser
            ? 'bg-titan-accent text-white'
            : 'bg-titan-surface text-titan-text'
        }`}
      >
        {!isUser && message.thinking && (
          <ThinkingBlock thinking={message.thinking} isStreaming={isStreaming} />
        )}
        {!isUser && message.phases && message.phases.length > 0 && (
          <PhaseActivity phases={message.phases} isStreaming={isStreaming} />
        )}
        {!isUser && message.toolCalls && message.toolCalls.length > 0 && (
          <ToolActivity toolCalls={message.toolCalls} isStreaming={isStreaming} />
        )}
        {isUser ? (
          <span className="whitespace-pre-wrap break-words">{message.content}</span>
        ) : (
          <div className="prose prose-invert max-w-none break-words">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
          </div>
        )}
        {isStreaming && !isUser && (
          <span className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-titan-text-muted align-middle" />
        )}
        {!isUser && !isStreaming && message.content && onSpeak && (
          <div className="mt-2 flex justify-end">
            <button
              onClick={() => isSpeaking ? onStopSpeaking?.() : onSpeak(message.content)}
              className={`flex items-center gap-1 rounded-lg px-2 py-1 text-[10px] transition-colors ${
                isSpeaking
                  ? 'text-red-400 hover:text-red-300 hover:bg-red-500/10'
                  : 'text-titan-text-muted hover:text-titan-text hover:bg-titan-bg/50'
              }`}
              title={isSpeaking ? 'Stop speaking' : 'Read aloud'}
            >
              {isSpeaking ? <VolumeX size={12} /> : <Volume2 size={12} />}
              <span>{isSpeaking ? 'Stop' : 'Speak'}</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
