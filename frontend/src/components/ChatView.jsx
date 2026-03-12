import { useEffect, useRef } from 'react';
import MessageBubble from './MessageBubble';
import MessageInput from './MessageInput';
import ModelBadge from './ModelBadge';
import StatusIndicator from './StatusIndicator';
import { MessageSquare } from 'lucide-react';

export default function ChatView({
  conversation,
  isStreaming,
  deepMode,
  onSend,
  onStop,
  onToggleDeep,
  isRecording,
  onStartRecording,
  onStopRecording,
  isSpeaking,
  onSpeak,
  onStopSpeaking,
  timeContext,
}) {
  const scrollRef = useRef(null);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [conversation?.messages]);

  if (!conversation) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4 text-titan-text-muted">
        <MessageSquare size={48} strokeWidth={1.5} />
        <p className="text-lg">Select or start a new conversation</p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col">
      {/* Top bar */}
      <div className="flex items-center justify-between border-b border-titan-border px-4 py-2.5">
        <ModelBadge />
        <StatusIndicator timeContext={timeContext} />
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-4 py-6">
        <div className="mx-auto flex max-w-3xl flex-col gap-6">
          {conversation.messages.length === 0 && (
            <div className="flex flex-col items-center justify-center gap-3 pt-20 text-titan-text-muted">
              <MessageSquare size={40} strokeWidth={1.5} />
              <p>How can I help you today?</p>
            </div>
          )}
          {conversation.messages.map((msg, i) => (
            <MessageBubble
              key={msg.id}
              message={msg}
              isStreaming={
                isStreaming &&
                i === conversation.messages.length - 1 &&
                msg.role === 'assistant'
              }
              onSpeak={onSpeak}
              isSpeaking={isSpeaking}
              onStopSpeaking={onStopSpeaking}
            />
          ))}
        </div>
      </div>

      {/* Input */}
      <MessageInput
        onSend={onSend}
        onStop={onStop}
        isStreaming={isStreaming}
        disabled={false}
        deepMode={deepMode}
        onToggleDeep={onToggleDeep}
        isRecording={isRecording}
        onStartRecording={onStartRecording}
        onStopRecording={onStopRecording}
      />
    </div>
  );
}
