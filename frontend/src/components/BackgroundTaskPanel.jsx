import { useState } from 'react';
import {
  ListTodo,
  ChevronDown,
  ChevronRight,
  Loader2,
  CheckCircle2,
  XCircle,
  Clock,
  Send,
  X,
} from 'lucide-react';

function formatDuration(startedAt, completedAt) {
  if (!startedAt) return '';
  const end = completedAt || Date.now() / 1000;
  const secs = Math.floor(end - startedAt);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${secs % 60}s`;
  return `${Math.floor(mins / 60)}h ${mins % 60}m`;
}

const STATUS_ICON = {
  queued: <Clock size={13} className="text-yellow-400" />,
  running: <Loader2 size={13} className="text-blue-400 animate-spin" />,
  completed: <CheckCircle2 size={13} className="text-emerald-400" />,
  failed: <XCircle size={13} className="text-red-400" />,
  cancelled: <X size={13} className="text-gray-400" />,
};

export default function BackgroundTaskPanel({ tasks, onSubmit, onCancel }) {
  const [collapsed, setCollapsed] = useState(false);
  const [input, setInput] = useState('');
  const [expandedId, setExpandedId] = useState(null);

  const activeCount = (tasks || []).filter(
    (t) => t.status === 'queued' || t.status === 'running'
  ).length;

  function handleSubmit(e) {
    e.preventDefault();
    const desc = input.trim();
    if (!desc || !onSubmit) return;
    onSubmit(desc);
    setInput('');
  }

  return (
    <div className="border-t border-titan-border">
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="flex w-full items-center gap-2 px-3 py-2 text-xs font-medium text-titan-text-muted hover:text-titan-text transition-colors"
      >
        <ListTodo size={13} className="text-blue-400" />
        <span>Background Tasks</span>
        {activeCount > 0 && (
          <span className="ml-1 rounded-full bg-blue-500/20 px-1.5 py-0.5 text-[10px] text-blue-400">
            {activeCount}
          </span>
        )}
        <span className="ml-auto">
          {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
        </span>
      </button>

      {!collapsed && (
        <div className="px-3 pb-2 space-y-2">
          {/* Submit input */}
          <form onSubmit={handleSubmit} className="flex gap-1">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="Describe a task..."
              className="flex-1 rounded-md border border-titan-border bg-titan-surface px-2 py-1 text-[11px] text-titan-text placeholder-titan-text-muted outline-none focus:border-titan-accent"
            />
            <button
              type="submit"
              disabled={!input.trim()}
              className="rounded-md bg-titan-accent/80 px-1.5 py-1 text-white disabled:opacity-30 hover:bg-titan-accent transition-colors"
            >
              <Send size={11} />
            </button>
          </form>

          {/* Task list */}
          <div className="max-h-52 overflow-y-auto space-y-1">
            {(tasks || []).map((task) => (
              <div
                key={task.id}
                className="rounded-md bg-titan-surface/50 px-2.5 py-1.5"
              >
                <div className="flex items-start gap-1.5">
                  {STATUS_ICON[task.status] || STATUS_ICON.queued}
                  <div className="flex-1 min-w-0">
                    <button
                      onClick={() =>
                        setExpandedId(expandedId === task.id ? null : task.id)
                      }
                      className="text-left text-[11px] leading-snug text-titan-text-muted truncate block w-full"
                    >
                      {task.description}
                    </button>
                    <div className="flex items-center gap-2 mt-0.5">
                      <span className="text-[10px] text-titan-text-muted/50">
                        {task.status}
                        {task.started_at &&
                          ` - ${formatDuration(task.started_at, task.completed_at)}`}
                      </span>
                      {task.status === 'queued' && onCancel && (
                        <button
                          onClick={() => onCancel(task.id)}
                          className="text-[10px] text-red-400 hover:text-red-300"
                        >
                          cancel
                        </button>
                      )}
                    </div>
                  </div>
                </div>

                {/* Expanded details */}
                {expandedId === task.id && (
                  <div className="mt-1.5 pl-5 space-y-0.5 border-t border-titan-border/50 pt-1.5">
                    {task.result && (
                      <p className="text-[10px] text-emerald-400/80 whitespace-pre-wrap">
                        {task.result}
                      </p>
                    )}
                    {task.error && (
                      <p className="text-[10px] text-red-400/80">{task.error}</p>
                    )}
                    {(task.progress || []).slice(-5).map((step, i) => (
                      <div
                        key={i}
                        className="text-[10px] text-titan-text-muted/60"
                      >
                        <span className="font-mono">[{step.type}]</span>{' '}
                        {step.content || step.tool || ''}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
