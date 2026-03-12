import { useEffect, useRef } from 'react';
import { fetchVoiceEvents } from '../api/client';

/**
 * Polls /v1/voice/events for voice_input commands.
 *
 * When the backend's always-on listener hears a wake-word command,
 * it emits a voice_input event with the transcribed text. This hook
 * picks it up and submits it through the normal chat pipeline via
 * `onSubmit` — producing identical rendering (tool steps, streaming
 * tokens, etc.) to a message typed by the user.
 *
 * After the chat stream finishes, the response is automatically spoken
 * using the same speak() function as the per-message speak button.
 */
export function useVoiceEvents({ onSubmit, speak, isStreaming, activeConversation, createConversation, activeId }) {
  const activeIdRef = useRef(activeId);
  const voiceInitiatedRef = useRef(false);
  const wasStreamingRef = useRef(false);

  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  // Watch for streaming to finish after a voice-initiated message
  useEffect(() => {
    if (isStreaming) {
      wasStreamingRef.current = true;
      return;
    }

    // isStreaming just went false
    if (!wasStreamingRef.current || !voiceInitiatedRef.current) {
      wasStreamingRef.current = false;
      return;
    }

    wasStreamingRef.current = false;
    voiceInitiatedRef.current = false;

    // Get the last assistant message content and speak it
    if (activeConversation?.messages?.length) {
      const msgs = activeConversation.messages;
      const lastMsg = msgs[msgs.length - 1];
      if (lastMsg?.role === 'assistant' && lastMsg.content?.trim()) {
        speak(lastMsg.content);
      }
    }
  }, [isStreaming, activeConversation, speak]);

  // Poll for voice input events
  useEffect(() => {
    const interval = setInterval(async () => {
      // Don't poll while a voice command is being processed
      if (voiceInitiatedRef.current) return;

      const events = await fetchVoiceEvents();
      if (!events.length) return;

      for (const ev of events) {
        if (ev.type !== 'voice_input') continue;

        const text = ev.text;
        if (!text) continue;

        // Mark this as a voice-initiated message so we auto-speak the response
        voiceInitiatedRef.current = true;

        // Ensure there's an active conversation
        if (!activeIdRef.current) {
          createConversation();
        }

        // Submit through the normal chat pipeline — identical rendering
        onSubmit(text);

        // Only process one voice_input per poll cycle
        break;
      }
    }, 1500);

    return () => clearInterval(interval);
  }, [onSubmit, createConversation]);
}
