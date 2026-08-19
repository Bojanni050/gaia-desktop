import React, { useState, useEffect } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import rehypeKatex from 'rehype-katex';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { Check, Copy } from 'lucide-react';
import mermaid from 'mermaid';

// Ported from gaia-web's src/gaia/conversation/Markdown.jsx so both clients
// render Gaia's replies identically — same libraries, same theme values.

mermaid.initialize({
  startOnLoad: false,
  theme: 'dark',
  securityLevel: 'loose',
  themeVariables: {
    background: '#1C1811',
    primaryColor: '#CBA36A',
    secondaryColor: '#221D15',
    tertiaryColor: '#2A241B',
    mainBkg: '#1C1811',
    nodeBorder: 'rgba(236, 230, 218, 0.13)',
    actorBorder: 'rgba(236, 230, 218, 0.13)',
    signalColor: '#ECE6DA',
    textColor: '#ECE6DA',
  }
});

function MermaidBlock({ value }) {
  const [svg, setSvg] = useState('');
  const [error, setError] = useState(null);

  useEffect(() => {
    let active = true;
    const id = `mermaid-${Math.random().toString(36).substring(2, 9)}`;

    try {
      mermaid.render(id, value)
        .then(({ svg: renderedSvg }) => {
          if (active) {
            setSvg(renderedSvg);
            setError(null);
          }
        })
        .catch((err) => {
          console.error('Mermaid render error:', err);
          if (active) {
            setError(String(err.message || err));
          }
        });
    } catch (err) {
      console.error('Mermaid sync error:', err);
      if (active) {
        setError(String(err.message || err));
      }
    }

    return () => {
      active = false;
      const tempDiv = document.getElementById(`d${id}`);
      if (tempDiv) tempDiv.remove();
    };
  }, [value]);

  if (error) {
    return (
      <div className="mermaid-error" data-testid="mermaid-error">
        <span className="mermaid-error-title">Diagram render error</span>
        <pre className="mermaid-error-details">{error}</pre>
      </div>
    );
  }

  if (!svg) {
    return <div className="mermaid-loading">Rendering diagram...</div>;
  }

  return (
    <div
      className="mermaid-block"
      data-testid="mermaid-block"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}

function CodeBlock({ language, value }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };
  return (
    <div className="code-block" data-testid="code-block">
      <div className="code-block-bar">
        <span className="code-lang">{language || 'text'}</span>
        <button className="code-copy" onClick={copy} data-testid="code-copy-btn" aria-label="Copy code">
          {copied ? <Check size={13} /> : <Copy size={13} />}
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <SyntaxHighlighter
        language={language || 'text'}
        style={oneDark}
        customStyle={{ margin: 0, background: 'transparent', fontSize: '0.85rem', padding: '14px 16px' }}
      >
        {value}
      </SyntaxHighlighter>
    </div>
  );
}

export default function Markdown({ children }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        code({ inline, className, children: c, ...props }) {
          const match = /language-(\w+)/.exec(className || '');
          const value = String(c).replace(/\n$/, '');
          if (!inline && match && match[1] === 'mermaid') {
            return <MermaidBlock value={value} />;
          }
          if (!inline && (match || value.includes('\n'))) {
            return <CodeBlock language={match ? match[1] : ''} value={value} />;
          }
          return <code className="inline-code" {...props}>{c}</code>;
        },
        a: ({ node, ...props }) => <a target="_blank" rel="noreferrer" {...props} />,
        table: ({ node, ...props }) => (
          <div className="md-table-wrap"><table {...props} /></div>
        ),
      }}
    >
      {children}
    </ReactMarkdown>
  );
}
