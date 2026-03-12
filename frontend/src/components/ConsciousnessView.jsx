import { useState, useEffect, useCallback } from 'react';
import { Brain, Shield, Target, BookOpen, User, BarChart3, Clock, RefreshCw, MessageCircle } from 'lucide-react';
import { getThoughts, getConsciousnessLedgers, getConsciousnessModes } from '../api/client';
import SecurityAlertPanel from './SecurityAlertPanel';
import ConsciousnessDialogue from './ConsciousnessDialogue';

const SUB_TABS = [
  { id: 'all',       label: 'All Thoughts', icon: Brain },
  { id: 'persona',   label: 'Persona',      icon: User },
  { id: 'knowledge', label: 'Knowledge',    icon: BookOpen },
  { id: 'goals',     label: 'Goals',        icon: Target },
  { id: 'security',  label: 'Security',     icon: Shield },
  { id: 'modes',     label: 'Modes',        icon: BarChart3 },
  { id: 'dialogue',  label: 'Dialogue',     icon: MessageCircle },
];

const CATEGORY_COLORS = {
  self_awareness: 'bg-purple-500/20 text-purple-300',
  environment:    'bg-green-500/20 text-green-300',
  capabilities:   'bg-blue-500/20 text-blue-300',
  temporal:       'bg-amber-500/20 text-amber-300',
  planning:       'bg-cyan-500/20 text-cyan-300',
  memory:         'bg-indigo-500/20 text-indigo-300',
  curiosity:      'bg-pink-500/20 text-pink-300',
  observation:    'bg-teal-500/20 text-teal-300',
  security:       'bg-red-500/20 text-red-300',
  boot:           'bg-yellow-500/20 text-yellow-300',
  learn:          'bg-emerald-500/20 text-emerald-300',
  act:            'bg-orange-500/20 text-orange-300',
  reflect:        'bg-violet-500/20 text-violet-300',
  research:       'bg-sky-500/20 text-sky-300',
};

function timeAgo(timestamp) {
  const secs = Math.floor(Date.now() / 1000 - timestamp);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

function CategoryBadge({ category }) {
  const colors = CATEGORY_COLORS[category] || 'bg-gray-500/20 text-gray-300';
  return (
    <span className={`inline-block rounded-full px-2 py-0.5 text-[10px] font-medium ${colors}`}>
      {category}
    </span>
  );
}

function ThoughtCard({ thought }) {
  const isAlert = thought.category === 'security' &&
    /unusual|unexpected|suspicious|alert|anomaly/i.test(thought.text);

  return (
    <div className={`rounded-lg border px-3 py-2.5 ${
      isAlert
        ? 'border-red-500/50 bg-red-500/5'
        : 'border-titan-border bg-titan-surface/50'
    }`}>
      <div className="flex items-center gap-2 mb-1">
        <CategoryBadge category={thought.category} />
        <span className="text-[10px] text-titan-text-muted flex items-center gap-1">
          <Clock size={10} />
          {timeAgo(thought.timestamp)}
        </span>
      </div>
      <p className="text-xs leading-relaxed text-titan-text opacity-90">{thought.text}</p>
    </div>
  );
}

function LedgerView({ content, emptyMessage }) {
  if (!content || content.trim().length === 0) {
    return (
      <div className="flex items-center justify-center h-48 text-sm text-titan-text-muted">
        {emptyMessage || 'No entries yet'}
      </div>
    );
  }

  // Split by common ledger headers (lines starting with patterns like "Identity —", "Network Scan —", etc.)
  const entries = content.split(/\n(?=\S.*? — \d{4}-\d{2}-\d{2})/).filter(Boolean);

  return (
    <div className="space-y-2">
      {entries.map((entry, i) => {
        const headerMatch = entry.match(/^(.+? — \d{4}-\d{2}-\d{2} \d{2}:\d{2})\n?([\s\S]*)/);
        const header = headerMatch ? headerMatch[1] : null;
        const body = headerMatch ? headerMatch[2]?.trim() : entry.trim();
        const isSecurityAlert = /unusual|unexpected|suspicious|alert|anomaly/i.test(body);

        return (
          <div key={i} className={`rounded-lg border px-3 py-2.5 ${
            isSecurityAlert ? 'border-red-500/40 bg-red-500/5' : 'border-titan-border bg-titan-surface/50'
          }`}>
            {header && (
              <div className="text-[10px] font-medium text-titan-text-muted mb-1">{header}</div>
            )}
            <p className="text-xs leading-relaxed text-titan-text whitespace-pre-wrap">{body}</p>
          </div>
        );
      })}
    </div>
  );
}

function ModeWeightBar({ mode, weight, maxWeight }) {
  const pct = maxWeight > 0 ? (weight / maxWeight) * 100 : 0;
  return (
    <div className="flex items-center gap-2">
      <span className="w-24 text-xs text-titan-text-muted capitalize">{mode}</span>
      <div className="flex-1 h-4 rounded-full bg-titan-bg overflow-hidden">
        <div
          className="h-full rounded-full bg-titan-accent transition-all duration-500"
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className="w-12 text-right text-xs text-titan-text-muted">{(weight * 100).toFixed(0)}%</span>
    </div>
  );
}

function ModesView({ modeWeights, modeHistory }) {
  const maxWeight = Math.max(...Object.values(modeWeights || {}), 0.01);

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-xs font-medium text-titan-text-muted mb-2">Weight Distribution</h3>
        <div className="space-y-1.5">
          {Object.entries(modeWeights || {}).sort(([,a],[,b]) => b - a).map(([mode, weight]) => (
            <ModeWeightBar key={mode} mode={mode} weight={weight} maxWeight={maxWeight} />
          ))}
        </div>
      </div>

      {modeHistory && modeHistory.length > 0 && (
        <div>
          <h3 className="text-xs font-medium text-titan-text-muted mb-2">Recent Executions</h3>
          <div className="space-y-1">
            {modeHistory.slice(-15).reverse().map((entry, i) => (
              <div key={i} className="flex items-center gap-2 text-xs text-titan-text-muted">
                <span className="w-20 capitalize">{entry.mode}</span>
                <span className={entry.success ? 'text-green-400' : 'text-red-400'}>
                  {entry.success ? 'ok' : 'fail'}
                </span>
                {entry.timestamp && (
                  <span className="text-[10px]">{timeAgo(entry.timestamp)}</span>
                )}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default function ConsciousnessView({ thoughts: liveThoughts, securityAlerts = [], dialogueMessages = [] }) {
  const [activeSubTab, setActiveSubTab] = useState('all');
  const [allThoughts, setAllThoughts] = useState([]);
  const [ledgers, setLedgers] = useState({});
  const [modes, setModes] = useState({ mode_weights: {}, mode_history: [] });
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [thoughtsRes, ledgersRes, modesRes] = await Promise.allSettled([
        getThoughts(100),
        getConsciousnessLedgers(),
        getConsciousnessModes(),
      ]);
      if (thoughtsRes.status === 'fulfilled') setAllThoughts(thoughtsRes.value);
      if (ledgersRes.status === 'fulfilled') setLedgers(ledgersRes.value);
      if (modesRes.status === 'fulfilled') setModes(modesRes.value);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  // Merge live SSE thoughts with fetched thoughts
  const mergedThoughts = [...allThoughts];
  if (liveThoughts) {
    for (const t of liveThoughts) {
      if (!mergedThoughts.some(e => e.timestamp === t.timestamp && e.text === t.text)) {
        mergedThoughts.push(t);
      }
    }
  }
  mergedThoughts.sort((a, b) => b.timestamp - a.timestamp);

  const totalThoughts = mergedThoughts.length;
  const categories = {};
  for (const t of mergedThoughts) {
    categories[t.category] = (categories[t.category] || 0) + 1;
  }

  return (
    <div className="flex flex-1 flex-col h-full overflow-hidden bg-titan-bg">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-titan-border px-4 py-3">
        <div className="flex items-center gap-2">
          <Brain size={18} className="text-purple-400" />
          <h1 className="text-sm font-semibold">Consciousness</h1>
          <span className="text-xs text-titan-text-muted">
            {totalThoughts} thought{totalThoughts !== 1 ? 's' : ''}
          </span>
        </div>
        <button
          onClick={refresh}
          disabled={loading}
          className="flex items-center gap-1 rounded-lg px-2 py-1 text-xs text-titan-text-muted hover:text-titan-text transition-colors"
        >
          <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
          Refresh
        </button>
      </div>

      {/* Sub-tabs */}
      <div className="flex border-b border-titan-border overflow-x-auto">
        {SUB_TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            onClick={() => setActiveSubTab(id)}
            className={`flex items-center gap-1.5 px-3 py-2 text-xs font-medium whitespace-nowrap transition-colors ${
              activeSubTab === id
                ? 'text-titan-accent border-b-2 border-titan-accent'
                : 'text-titan-text-muted hover:text-titan-text'
            }`}
          >
            <Icon size={12} />
            {label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-4 space-y-2">
        {activeSubTab === 'all' && (
          mergedThoughts.length > 0 ? (
            mergedThoughts.map((thought, i) => (
              <ThoughtCard key={`${thought.timestamp}-${i}`} thought={thought} />
            ))
          ) : (
            <div className="flex items-center justify-center h-48 text-sm text-titan-text-muted">
              No thoughts yet — consciousness is warming up
            </div>
          )
        )}

        {activeSubTab === 'persona' && (
          <LedgerView content={ledgers.persona} emptyMessage="No persona entries yet" />
        )}

        {activeSubTab === 'knowledge' && (
          <LedgerView content={ledgers.knowledge} emptyMessage="No knowledge entries yet" />
        )}

        {activeSubTab === 'goals' && (
          <LedgerView content={ledgers.goals} emptyMessage="No goal entries yet" />
        )}

        {activeSubTab === 'security' && (
          <SecurityAlertPanel liveAlerts={securityAlerts} />
        )}

        {activeSubTab === 'modes' && (
          <ModesView modeWeights={modes.mode_weights} modeHistory={modes.mode_history} />
        )}

        {activeSubTab === 'dialogue' && (
          <ConsciousnessDialogue messages={dialogueMessages} />
        )}
      </div>

      {/* Footer stats */}
      <div className="flex items-center gap-4 border-t border-titan-border px-4 py-2 text-[10px] text-titan-text-muted">
        <span>{totalThoughts} total thoughts</span>
        <span className="h-3 w-px bg-titan-border" />
        {Object.entries(categories).sort(([,a],[,b]) => b - a).slice(0, 5).map(([cat, count]) => (
          <span key={cat} className="capitalize">{cat}: {count}</span>
        ))}
      </div>
    </div>
  );
}
