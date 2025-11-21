'use client';

import { useRef, FormEvent } from 'react';
import Navbar from '@/components/Navbar';
import Button from '@/components/Button';
import Input from '@/components/Input';
import ProgressBar from '@/components/ProgressBar';
import { useUpload } from '@/context/UploadContext';
import { formatFileSize } from '@/utils/format';

export default function Home() {
  const {
    files,
    setFiles,
    tags,
    setTags,
    isUploading,
    progress,
    results,
    error,
    setError,
    uploadStatus,
    startUpload,
    clearUploads
  } = useUpload();

  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) {
      setFiles(Array.from(e.target.files));
      // We don't clear results here anymore to allow persistence across navigation
      // But if user selects new files, maybe we should clear previous results?
      // Let's keep previous results until they hit upload or clear explicitly
      setError(null);
    }
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    await startUpload();
    if (fileInputRef.current) fileInputRef.current.value = '';
  };

  const handleClear = () => {
    clearUploads();
    if (fileInputRef.current) fileInputRef.current.value = '';
  };

  return (
    <div className="min-h-screen bg-background p-10 font-sans text-foreground">
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

          <div className="flex flex-col gap-2">
            <label htmlFor="fileInput" className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70 text-foreground">
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
              className="flex w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
            />
            <p className="text-[0.8rem] text-muted-foreground">Select one or more video files</p>
          </div>

          {files.length > 0 && (
            <div className="rounded-md border border-border bg-card p-4 text-card-foreground">
              <div className="flex flex-col gap-2">
                {files.map((file, idx) => (
                  <div key={idx} className="flex items-center justify-between border-b border-border py-2 last:border-0">
                    <div>
                      <div className="font-medium">{file.name}</div>
                      <div className="text-xs text-muted-foreground">{formatFileSize(file.size)}</div>
                    </div>
                    <div className="text-xs text-muted-foreground">Pending</div>
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
              onClick={handleClear}
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
          <div className="mt-6 rounded-md bg-destructive/15 p-4 text-destructive">
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
                  className={`rounded-md border p-4 ${result.success ? 'border-green-500/20 bg-green-500/10' : 'border-destructive/20 bg-destructive/10'}`}
                >
                  <div className="font-medium">{result.file}</div>
                  {result.success && result.data ? (
                    <div className="mt-2 text-sm">
                      <span className="text-green-500">✓ Uploaded successfully!</span>
                      <div className="mt-1">
                        <span className="font-medium text-muted-foreground">Playlist URL: </span>
                        <a 
                          href={result.data.playlist_url} 
                          target="_blank" 
                          rel="noopener noreferrer"
                          className="text-primary hover:underline break-all"
                        >
                          {result.data.playlist_url}
                        </a>
                      </div>
                    </div>
                  ) : (
                    <div className="mt-2 text-sm text-destructive">
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
