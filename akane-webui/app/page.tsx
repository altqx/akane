'use client';

import { useState, useRef, FormEvent } from 'react';
import Navbar from '@/components/Navbar';
import Button from '@/components/Button';
import Input from '@/components/Input';
import ProgressBar from '@/components/ProgressBar';

interface UploadResult {
  file: string;
  success: boolean;
  data?: {
    playlist_url: string;
    upload_id: string;
  };
  error?: string;
}

interface ProgressData {
  percentage: number;
  stage: string;
  current_chunk: number;
  total_chunks: number;
}

export default function Home() {
  const [files, setFiles] = useState<File[]>([]);
  const [tags, setTags] = useState('');
  const [isUploading, setIsUploading] = useState(false);
  const [progress, setProgress] = useState<ProgressData | null>(null);
  const [results, setResults] = useState<UploadResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [uploadStatus, setUploadStatus] = useState<string>('');

  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) {
      setFiles(Array.from(e.target.files));
      setResults([]);
      setError(null);
      setProgress(null);
    }
  };

  const formatFileSize = (bytes: number) => {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
  };

  const pollProgress = async (uploadId: string) => {
    try {
      const token = localStorage.getItem('admin_token');
      const res = await fetch(`/api/progress/${uploadId}`, {
        headers: {
          'Authorization': `Bearer ${token}`
        }
      });
      if (!res.ok) return;
      const data = await res.json();
      if (data) {
        setProgress(data);
      }
    } catch (err) {
      console.error('Progress poll error:', err);
    }
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (files.length === 0) {
      setError('Please select at least one video file.');
      return;
    }

    setIsUploading(true);
    setResults([]);
    setError(null);
    
    const newResults: UploadResult[] = [];

    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      setUploadStatus(`Uploading ${i + 1} of ${files.length}: ${file.name}`);
      setProgress(null);

      // Generate a client-side ID for progress tracking
      const uploadId = crypto.randomUUID();

      // Start polling immediately
      const pollInterval = setInterval(() => {
        pollProgress(uploadId);
      }, 500);

      try {
        const formData = new FormData();
        formData.append('file', file);
        formData.append('name', file.name.replace(/\.[^/.]+$/, ''));
        if (tags.trim()) {
          formData.append('tags', tags.trim());
        }

        // Start upload request with X-Upload-ID header
        const token = localStorage.getItem('admin_token');
        const res = await fetch('/api/upload', {
          method: 'POST',
          headers: {
            'X-Upload-ID': uploadId,
            'Authorization': `Bearer ${token}`
          },
          body: formData,
        });

        if (!res.ok) {
          const text = await res.text();
          throw new Error(text || 'Upload failed');
        }

        const data = await res.json();
        
        newResults.push({
          file: file.name,
          success: true,
          data: data
        });

      } catch (err: unknown) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        newResults.push({
          file: file.name,
          success: false,
          error: errorMessage
        });
      } finally {
        clearInterval(pollInterval);
      }
      
    }

    setResults(newResults);
    setIsUploading(false);
    setUploadStatus('');
    setProgress(null);
    setFiles([]); // Clear files on success? Or keep them? Let's clear.
    if (fileInputRef.current) fileInputRef.current.value = '';
  };

  return (
    <div className="min-h-screen bg-white p-10 font-sans text-gray-900">
      <div className="mx-auto max-w-2xl">
        <h1 className="mb-6 text-3xl font-bold">Akane Video Uploader</h1>
        
        <Navbar />

        <form onSubmit={handleSubmit} className="flex flex-col gap-6">
          <Input
            id="tags"
            label="Tags (optional)"
            placeholder="gaming, tutorial, 4k"
            hint="Separate tags with commas (applied to all files)"
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            disabled={isUploading}
          />

          <div className="flex flex-col gap-1">
            <label htmlFor="fileInput" className="text-sm font-medium text-gray-700">
              Video Files *
            </label>
            <input
              ref={fileInputRef}
              type="file"
              id="fileInput"
              accept="video/*,.mkv"
              multiple
              required
              onChange={handleFileChange}
              disabled={isUploading}
              className="block w-full text-sm text-gray-500 file:mr-4 file:py-2 file:px-4 file:rounded file:border-0 file:text-sm file:font-semibold file:bg-gray-100 file:text-gray-700 hover:file:bg-gray-200"
            />
            <p className="text-xs text-gray-500">Select one or more video files</p>
          </div>

          {files.length > 0 && (
            <div className="rounded bg-gray-50 p-4">
              <div className="flex flex-col gap-2">
                {files.map((file, idx) => (
                  <div key={idx} className="flex items-center justify-between border-b border-gray-200 py-2 last:border-0">
                    <div>
                      <div className="font-medium text-gray-800">{file.name}</div>
                      <div className="text-xs text-gray-500">{formatFileSize(file.size)}</div>
                    </div>
                    <div className="text-xs text-gray-500">Pending</div>
                  </div>
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-3">
            <Button type="submit" disabled={isUploading || files.length === 0} className="flex-1">
              {isUploading ? uploadStatus : 'Upload All'}
            </Button>
            <Button 
              type="button" 
              variant="secondary" 
              disabled={isUploading}
              onClick={() => {
                setFiles([]);
                setResults([]);
                setError(null);
                if (fileInputRef.current) fileInputRef.current.value = '';
              }}
              className="flex-1"
            >
              Clear
            </Button>
          </div>

          {progress && (
            <div className="mt-4">
              <ProgressBar 
                percentage={progress.percentage} 
                stage={progress.stage} 
                currentChunk={progress.current_chunk} 
                totalChunks={progress.total_chunks} 
              />
            </div>
          )}
        </form>

        {error && (
          <div className="mt-6 rounded bg-red-50 p-4 text-red-700">
            {error}
          </div>
        )}

        {results.length > 0 && (
          <div className="mt-8">
            <h3 className="mb-4 text-lg font-semibold">Upload Results</h3>
            <div className="flex flex-col gap-4">
              {results.map((result, idx) => (
                <div 
                  key={idx} 
                  className={`rounded p-4 ${result.success ? 'bg-green-50' : 'bg-red-50'}`}
                >
                  <div className="font-medium text-gray-900">{result.file}</div>
                  {result.success && result.data ? (
                    <div className="mt-2 text-sm">
                      <span className="text-green-700">✓ Uploaded successfully!</span>
                      <div className="mt-1">
                        <span className="font-medium text-gray-700">Playlist URL: </span>
                        <a 
                          href={result.data.playlist_url} 
                          target="_blank" 
                          rel="noopener noreferrer"
                          className="text-blue-600 hover:underline break-all"
                        >
                          {result.data.playlist_url}
                        </a>
                      </div>
                    </div>
                  ) : (
                    <div className="mt-2 text-sm text-red-700">
                      ✗ Failed: {result.error}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
