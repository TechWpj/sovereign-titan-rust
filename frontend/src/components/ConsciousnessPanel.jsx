import { useState } from 'react';
import { Brain, ChevronDown, ChevronRight } from 'lucide-react';

function timeAgo(timestamp) {
  const secs = Math.floor(Date.now() / 1000 - timestamp);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ago`;
}

export default function ConsciousnessPanel({ thoughts, onTabChange }) {
  const [collapsed, setCollapsed] = useState(false);

  if (!thoughts || thoughts.length === 0) return null;

  const visible = thoughts.slice(-10).reverse();

  return (
    <div className="border-t border-titan-border">
      <div className="flex items-center">
        <button
          onClick={() => setCollapsed(!collapsed)}
          className="flex flex-1 items-center gap-2 px-3 py-2 text-xs font-medium text-titan-text-muted hover:text-titan-text transition-colors"
        >
          <Brain size={13} className="text-purple-400" />
          <span
            onClick={(e) => { if (onTabChange) { e.stopPropagation(); onTabChange('consciousness'); } }}
            className={onTabChange ? 'cursor-pointer hover:text-purple-300' : ''}
            title={onTabChange ? 'Open full consciousness view' : undefined}
          >
            Inner Thoughts
          </span>
          <span className="ml-auto">
            {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
          </span>
        </button>
      </div>

      {!collapsed && (
        <div className="max-h-48 overflow-y-auto px-3 pb-2 space-y-1.5">
          {visible.map((thought, i) => (
            <div
              key={thought.timestamp + '-' + i}
              className="rounded-md bg-titan-surface/50 px-2.5 py-1.5 animate-fade-in"
            >
              <p className="text-[11px] leading-relaxed text-titan-text-muted opacity-80">
                {thought.text}
              </p>
              <span className="text-[10px] text-titan-text-muted/50">
                {timeAgo(thought.timestamp)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
