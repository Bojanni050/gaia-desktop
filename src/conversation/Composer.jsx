import React, { useRef, useState } from 'react';

export default function Composer({ onSend, busy }) {
  const [value, setValue] = useState('');
  const areaRef = useRef(null);

  const submit = () => {
    const text = value.trim();
    if (!text || busy) return;
    onSend(text);
    setValue('');
    if (areaRef.current) areaRef.current.style.height = 'auto';
  };

  const handleKeyDown = (event) => {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      submit();
    }
  };

  const handleChange = (event) => {
    setValue(event.target.value);
    const el = event.target;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  };

  return (
    <div className="composer">
      <textarea
        ref={areaRef}
        className="composer-input"
        value={value}
        placeholder={busy ? 'Gaia is thinking…' : 'Say something'}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        rows={1}
        data-testid="composer"
      />
      <button className="composer-send" onClick={submit} disabled={busy || !value.trim()}>
        Send
      </button>
    </div>
  );
}
