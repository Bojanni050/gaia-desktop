/**
 * Library — files stored server-side in Gaia Cloud (never on this
 * device). Upload picks a local file via a native dialog and hands its
 * path to Rust, which streams the bytes to Gaia Server; nothing here
 * reads or interprets file content. Same calm-overlay pattern as
 * SettingsPanel — no confirmation dialog on delete, matching how thread
 * deletion already works in the sidebar.
 */
import React, { useEffect, useState } from 'react';
import { Download, Trash2, Upload } from 'lucide-react';
import { libraryApi } from '../server/api';
import { L } from '../lib/lexicon';

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(iso) {
  try {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  } catch (_) {
    return '';
  }
}

export default function LibraryPanel({ onClose }) {
  const [files, setFiles] = useState(null);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState(null);
  const [busyId, setBusyId] = useState(null);

  const load = () => {
    libraryApi
      .listFiles()
      .then(setFiles)
      .catch(() => setFiles([]));
  };

  useEffect(load, []);

  const handleUpload = async () => {
    setError(null);
    setUploading(true);
    try {
      const uploaded = await libraryApi.pickAndUploadFile();
      if (uploaded) setFiles((prev) => [uploaded, ...(prev || [])]);
    } catch (_) {
      setError(L.libraryUploadFailed);
    } finally {
      setUploading(false);
    }
  };

  const handleDownload = async (file) => {
    setBusyId(file.id);
    setError(null);
    try {
      await libraryApi.downloadFile(file.id, file.filename);
    } catch (_) {
      setError(L.libraryDownloadFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleDelete = async (file) => {
    setBusyId(file.id);
    setError(null);
    try {
      await libraryApi.deleteFile(file.id);
      setFiles((prev) => (prev || []).filter((f) => f.id !== file.id));
    } catch (_) {
      setError(L.libraryDeleteFailed);
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel library-panel" onClick={(e) => e.stopPropagation()}>
        <h2>{L.libraryTitle}</h2>
        <p className="library-hint">{L.libraryHint}</p>

        <button className="primary library-upload-btn" onClick={handleUpload} disabled={uploading}>
          <Upload size={15} /> {uploading ? L.libraryUploading : L.libraryUpload}
        </button>

        {error && <div className="test-result test-error">{error}</div>}

        {files === null ? (
          <p className="library-hint">…</p>
        ) : files.length === 0 ? (
          <p className="library-empty">{L.libraryEmpty}</p>
        ) : (
          <div className="library-list">
            {files.map((file) => (
              <div key={file.id} className="library-item">
                <div className="library-item-info">
                  <span className="library-item-name">{file.filename}</span>
                  <span className="library-item-meta">
                    {formatSize(file.size)} · {formatDate(file.uploadedAt)}
                  </span>
                </div>
                <div className="library-item-actions">
                  <button
                    className="library-icon-btn"
                    onClick={() => handleDownload(file)}
                    disabled={busyId === file.id}
                    aria-label={L.libraryDownload}
                    title={L.libraryDownload}
                  >
                    <Download size={14} />
                  </button>
                  <button
                    className="library-icon-btn"
                    onClick={() => handleDelete(file)}
                    disabled={busyId === file.id}
                    aria-label={L.libraryDelete}
                    title={L.libraryDelete}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        <div className="settings-actions">
          <button onClick={onClose}>{L.settingsClose}</button>
        </div>
      </div>
    </div>
  );
}
