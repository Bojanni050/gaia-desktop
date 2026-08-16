import React from 'react';

export default function MessageView({ message }) {
  if (message.role === 'user') {
    return (
      <div className="message message-user" data-testid="message-user">
        <div className="message-body">{message.content}</div>
      </div>
    );
  }
  return (
    <div className={`message message-assistant${message.failed ? ' message-failed' : ''}`}>
      <div className="message-body">{message.content}</div>
    </div>
  );
}
