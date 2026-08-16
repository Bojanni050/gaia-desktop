import React, { useEffect, useRef, useState } from 'react';
import MessageView from './MessageView';

export default function Conversation({ thread, busy, presence }) {
  const scrollRef = useRef(null);
  const [pinned, setPinned] = useState(true);
  const messages = thread?.messages || [];

  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinned) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages.length, busy, pinned]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    setPinned(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
  };

  return (
    <div className="conversation-scroll" ref={scrollRef} onScroll={handleScroll}>
      {messages.length === 0 && <Welcome presence={presence} />}
      {messages.map((message) => (
        <MessageView key={message.id} message={message} />
      ))}
      {busy && (
        <div className="message message-assistant">
          <div className="message-body message-thinking">…</div>
        </div>
      )}
    </div>
  );
}

function Welcome({ presence }) {
  return (
    <div className="welcome">
      <p className="welcome-line">Gaia is here.</p>
      {presence === 'localOnly' && (
        <p className="welcome-hint">She is not connected to her server yet — add one in settings, and she can speak.</p>
      )}
    </div>
  );
}
