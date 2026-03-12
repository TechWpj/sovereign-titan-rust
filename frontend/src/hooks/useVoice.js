import { useState, useRef, useCallback } from 'react';
import { speakText, transcribeAudio } from '../api/client';

export function useVoice() {
  const [isRecording, setIsRecording] = useState(false);
  const [isSpeaking, setIsSpeaking] = useState(false);
  const mediaRecorderRef = useRef(null);
  const chunksRef = useRef([]);
  const audioRef = useRef(null);

  const startRecording = useCallback(async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const mediaRecorder = new MediaRecorder(stream, {
        mimeType: MediaRecorder.isTypeSupported('audio/webm') ? 'audio/webm' : 'audio/mp4',
      });
      chunksRef.current = [];

      mediaRecorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };

      mediaRecorderRef.current = mediaRecorder;
      mediaRecorder.start();
      setIsRecording(true);
    } catch (err) {
      console.error('Microphone access denied:', err);
      throw err;
    }
  }, []);

  const stopRecording = useCallback(async () => {
    return new Promise((resolve, reject) => {
      const recorder = mediaRecorderRef.current;
      if (!recorder || recorder.state === 'inactive') {
        setIsRecording(false);
        resolve('');
        return;
      }

      recorder.onstop = async () => {
        setIsRecording(false);
        const blob = new Blob(chunksRef.current, { type: recorder.mimeType });
        // Stop all tracks
        recorder.stream.getTracks().forEach((t) => t.stop());
        mediaRecorderRef.current = null;

        try {
          const text = await transcribeAudio(blob);
          resolve(text);
        } catch (err) {
          console.error('Transcription failed:', err);
          reject(err);
        }
      };

      recorder.stop();
    });
  }, []);

  const speak = useCallback(async (text) => {
    if (isSpeaking) return;
    setIsSpeaking(true);
    try {
      const blob = await speakText(text);
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      audioRef.current = audio;

      await new Promise((resolve, reject) => {
        audio.onended = () => {
          URL.revokeObjectURL(url);
          resolve();
        };
        audio.onerror = (e) => {
          URL.revokeObjectURL(url);
          reject(e);
        };
        audio.play();
      });
    } catch (err) {
      console.error('Speech playback failed:', err);
    } finally {
      setIsSpeaking(false);
      audioRef.current = null;
    }
  }, [isSpeaking]);

  const stopSpeaking = useCallback(() => {
    if (audioRef.current) {
      audioRef.current.pause();
      audioRef.current = null;
    }
    setIsSpeaking(false);
  }, []);

  return { isRecording, isSpeaking, startRecording, stopRecording, speak, stopSpeaking };
}
