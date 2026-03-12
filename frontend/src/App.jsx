import { useState } from 'react';
import Sidebar from './components/Sidebar';
import ChatView from './components/ChatView';
import AutomationView from './components/AutomationView';
import ConsciousnessView from './components/ConsciousnessView';
import { useConversations } from './hooks/useConversations';
import { useChat } from './hooks/useChat';
import { useVoice } from './hooks/useVoice';
import { useEventStream } from './hooks/useEventStream';
import { useVoiceEvents } from './hooks/useVoiceEvents';

export default function App() {
  const [activeTab, setActiveTab] = useState('chat');

  const {
    conversations,
    activeId,
    activeConversation,
    setActiveId,
    createConversation,
    deleteConversation,
    addMessage,
    updateLastMessage,
    addToolCallToLastMessage,
    addPhaseToLastMessage,
    addThinkingToLastMessage,
  } = useConversations();

  const { isStreaming, deepMode, sendMessage, stopStreaming, toggleDeepMode } = useChat({
    addMessage,
    updateLastMessage,
    addToolCallToLastMessage,
    addPhaseToLastMessage,
    addThinkingToLastMessage,
  });

  const { isRecording, isSpeaking, startRecording, stopRecording, speak, stopSpeaking } = useVoice();

  const { thoughts, tasks, timeContext, securityAlerts, dialogueMessages, submitTask, cancelTask } = useEventStream();

  function handleSend(content) {
    const conv = activeConversation || createConversation();
    sendMessage(content, conv);
  }

  useVoiceEvents({
    onSubmit: handleSend,
    speak,
    isStreaming,
    activeConversation,
    createConversation,
    activeId,
  });

  function handleNewChat() {
    createConversation();
  }

  return (
    <div className="flex h-full">
      <Sidebar
        conversations={conversations}
        activeId={activeId}
        onSelect={setActiveId}
        onCreate={handleNewChat}
        onDelete={deleteConversation}
        thoughts={thoughts}
        tasks={tasks}
        onSubmitTask={submitTask}
        onCancelTask={cancelTask}
        activeTab={activeTab}
        onTabChange={setActiveTab}
      />
      {activeTab === 'chat' && (
        <ChatView
          conversation={activeConversation}
          isStreaming={isStreaming}
          deepMode={deepMode}
          onSend={handleSend}
          onStop={stopStreaming}
          onToggleDeep={toggleDeepMode}
          isRecording={isRecording}
          onStartRecording={startRecording}
          onStopRecording={stopRecording}
          isSpeaking={isSpeaking}
          onSpeak={speak}
          onStopSpeaking={stopSpeaking}
          timeContext={timeContext}
        />
      )}
      {activeTab === 'automations' && <AutomationView />}
      {activeTab === 'consciousness' && <ConsciousnessView thoughts={thoughts} securityAlerts={securityAlerts} dialogueMessages={dialogueMessages} />}
    </div>
  );
}
