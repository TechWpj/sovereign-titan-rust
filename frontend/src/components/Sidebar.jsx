import { useState } from 'react';
import { Plus, Trash2, MessageSquare, Key, Zap, Brain } from 'lucide-react';
import { getApiKey, saveApiKey } from '../lib/storage';
import ConsciousnessPanel from './ConsciousnessPanel';
import BackgroundTaskPanel from './BackgroundTaskPanel';
import VoicePanel from './VoicePanel';
import ProfilePanel from './ProfilePanel';

export default function Sidebar({
  conversations,
  activeId,
  onSelect,
  onCreate,
  onDelete,
  thoughts,
  tasks,
  onSubmitTask,
  onCancelTask,
  activeTab = 'chat',
  onTabChange,
}) {
  const [apiKey, setApiKey] = useState(() => getApiKey());
  const [hoveredId, setHoveredId] = useState(null);

  function handleApiKeyChange(e) {
    const val = e.target.value;
    setApiKey(val);
    saveApiKey(val);
  }

  const sorted = [...conversations].sort((a, b) => b.updatedAt - a.updatedAt);

  return (
    <div className="flex h-full w-64 shrink-0 flex-col border-r border-titan-border bg-titan-sidebar">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-titan-border px-4 py-4">
        <div className="h-7 w-7 rounded-lg bg-titan-accent flex items-center justify-center">
          <MessageSquare size={14} className="text-white" />
        </div>
        <span className="text-sm font-semibold tracking-wide">Sovereign Titan</span>
      </div>

      {/* Tab switcher */}
      {onTabChange && (
        <div className="flex border-b border-titan-border">
          <button
            onClick={() => onTabChange('chat')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-medium transition-colors ${
              activeTab === 'chat'
                ? 'text-titan-accent border-b-2 border-titan-accent'
                : 'text-titan-text-muted hover:text-titan-text'
            }`}
          >
            <MessageSquare size={12} />
            Chat
          </button>
          <button
            onClick={() => onTabChange('automations')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-medium transition-colors ${
              activeTab === 'automations'
                ? 'text-titan-accent border-b-2 border-titan-accent'
                : 'text-titan-text-muted hover:text-titan-text'
            }`}
          >
            <Zap size={12} />
            Automations
          </button>
          <button
            onClick={() => onTabChange('consciousness')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-2 text-xs font-medium transition-colors ${
              activeTab === 'consciousness'
                ? 'text-titan-accent border-b-2 border-titan-accent'
                : 'text-titan-text-muted hover:text-titan-text'
            }`}
          >
            <Brain size={12} />
            Mind
          </button>
        </div>
      )}

      {/* New chat button (chat tab only) */}
      {activeTab === 'chat' && (
        <div className="px-3 py-2">
          <button
            onClick={onCreate}
            className="flex w-full items-center gap-2 rounded-lg border border-titan-border px-3 py-2 text-sm text-titan-text-muted transition-colors hover:bg-titan-surface hover:text-titan-text"
          >
            <Plus size={16} />
            New Chat
          </button>
        </div>
      )}

      {/* Conversation list (chat tab only) */}
      <div className="flex-1 overflow-y-auto px-2 py-1">
        {activeTab === 'chat' && sorted.map((conv) => (
          <button
            key={conv.id}
            onClick={() => onSelect(conv.id)}
            onMouseEnter={() => setHoveredId(conv.id)}
            onMouseLeave={() => setHoveredId(null)}
            className={`group mb-0.5 flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors ${
              conv.id === activeId
                ? 'bg-titan-surface text-titan-text'
                : 'text-titan-text-muted hover:bg-titan-surface/50 hover:text-titan-text'
            }`}
          >
            <span className="truncate">{conv.title}</span>
            {hoveredId === conv.id && (
              <span
                role="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onDelete(conv.id);
                }}
                className="ml-1 shrink-0 rounded p-0.5 text-titan-text-muted hover:text-red-400"
              >
                <Trash2 size={14} />
              </span>
            )}
          </button>
        ))}
        {activeTab === 'automations' && (
          <div className="px-1 py-2 text-xs text-titan-text-muted text-center">
            Use the main panel to start automations
          </div>
        )}
      </div>

      {/* Consciousness panel */}
      <ConsciousnessPanel thoughts={thoughts} onTabChange={onTabChange} />

      {/* Background tasks panel */}
      <BackgroundTaskPanel tasks={tasks} onSubmit={onSubmitTask} onCancel={onCancelTask} />

      {/* Voice control panel */}
      <VoicePanel />

      {/* User profile panel */}
      <ProfilePanel />

      {/* API key */}
      <div className="border-t border-titan-border p-3">
        <div className="flex items-center gap-1.5 mb-1.5 text-xs text-titan-text-muted">
          <Key size={12} />
          API Key
        </div>
        <input
          type="password"
          value={apiKey}
          onChange={handleApiKeyChange}
          placeholder="Optional"
          className="w-full rounded-lg border border-titan-border bg-titan-surface px-3 py-1.5 text-xs text-titan-text placeholder-titan-text-muted outline-none focus:border-titan-accent"
        />
      </div>
    </div>
  );
}
