'use client'

import { useRef, FormEvent, useState } from 'react'
import Navbar from '@/components/Navbar'
import Button from '@/components/Button'
import Input from '@/components/Input'
import ProgressBar from '@/components/ProgressBar'
import ProcessingQueues from '@/components/ProcessingQueues'
import { useUpload } from '@/context/UploadContext'
import { formatFileSize } from '@/utils/format'

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
  } = useUpload()

  const fileInputRef = useRef<HTMLInputElement>(null)
  const [copiedId, setCopiedId] = useState<string | null>(null)

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) {
      setFiles(Array.from(e.target.files))
      setError(null)
    }
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    await startUpload()
    if (fileInputRef.current) fileInputRef.current.value = ''
  }

  const handleClear = () => {
    clearUploads()
    if (fileInputRef.current) fileInputRef.current.value = ''
  }

  const copyToClipboard = async (text: string, id: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopiedId(id)
      setTimeout(() => setCopiedId(null), 2000)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  return (
    <div className='min-h-screen bg-base-200 p-10 font-sans'>
      <div className='mx-auto max-w-3xl'>
        <div className='flex justify-between items-center mb-8'>
          <div>
            <h1 className='text-3xl font-bold tracking-tight'>Upload Video</h1>
            <p className='text-base-content/70 mt-1'>Upload and process your videos for streaming.</p>
          </div>
        </div>
        <Navbar />

        <ProcessingQueues />

        <div className='card bg-base-100 shadow-xl'>
          <div className='card-body'>
            <form onSubmit={handleSubmit} className='flex flex-col gap-6'>
              <Input
                id='tags'
                label='Tags (optional)'
                placeholder='gaming, tutorial, 4k'
                hint='Separate tags with commas (applied to all files)'
                value={tags}
                onChange={(e) => setTags(e.target.value)}
                disabled={isUploading}
              />

              <div className='form-control w-full'>
                <div className='label'>
                  <span className='label-text'>Video Files *</span>
                </div>
                <div className='relative'>
                  <input
                    ref={fileInputRef}
                    type='file'
                    id='fileInput'
                    accept='video/*,.mkv'
                    multiple
                    required
                    onChange={handleFileChange}
                    disabled={isUploading}
                    className='file-input file-input-bordered w-full h-32 pt-10 text-center'
                  />
                  <div className='absolute inset-0 pointer-events-none flex flex-col items-center justify-center text-base-content/50'>
                    <svg
                      xmlns='http://www.w3.org/2000/svg'
                      width='24'
                      height='24'
                      viewBox='0 0 24 24'
                      fill='none'
                      stroke='currentColor'
                      strokeWidth='2'
                      strokeLinecap='round'
                      strokeLinejoin='round'
                      className='mb-2 opacity-50'
                    >
                      <path d='M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4' />
                      <polyline points='17 8 12 3 7 8' />
                      <line x1='12' x2='12' y1='3' y2='15' />
                    </svg>
                    <span className='text-sm'>Click to select or drag and drop video files</span>
                  </div>
                </div>
                <div className='label'>
                  <span className='label-text-alt text-base-content/70'>Select one or more video files</span>
                </div>
              </div>

              {files.length > 0 && (
                <div className='bg-base-200 rounded-box p-4'>
                  <div className='flex flex-col gap-2'>
                    {files.map((file, idx) => (
                      <div
                        key={idx}
                        className='flex items-center justify-between border-b border-base-300 py-2 last:border-0'
                      >
                        <div className='flex items-center gap-3'>
                          <div className='h-8 w-8 rounded bg-primary/10 flex items-center justify-center text-primary'>
                            <svg
                              xmlns='http://www.w3.org/2000/svg'
                              width='16'
                              height='16'
                              viewBox='0 0 24 24'
                              fill='none'
                              stroke='currentColor'
                              strokeWidth='2'
                              strokeLinecap='round'
                              strokeLinejoin='round'
                            >
                              <path d='m22 8-6 4 6 4V8Z' />
                              <rect width='14' height='12' x='2' y='6' rx='2' ry='2' />
                            </svg>
                          </div>
                          <div>
                            <div className='font-medium text-sm'>{file.name}</div>
                            <div className='text-xs text-base-content/70'>{formatFileSize(file.size)}</div>
                          </div>
                        </div>
                        <div className='badge badge-secondary'>Pending</div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              <div className='flex gap-3 pt-2'>
                <Button type='submit' disabled={isUploading || files.length === 0} className='flex-1'>
                  {isUploading ? (
                    <span className='flex items-center gap-2'>
                      <span className='loading loading-spinner loading-sm'></span>
                      {uploadStatus}
                    </span>
                  ) : (
                    'Upload All'
                  )}
                </Button>
                <Button
                  type='button'
                  variant='secondary'
                  disabled={isUploading}
                  onClick={handleClear}
                  className='flex-1'
                >
                  Clear
                </Button>
              </div>

              {progress && (
                <div className='mt-2'>
                  <ProgressBar
                    percentage={progress.percentage}
                    stage={progress.stage}
                    currentChunk={progress.current_chunk}
                    totalChunks={progress.total_chunks}
                    details={progress.details}
                  />
                </div>
              )}
            </form>
          </div>
        </div>

        {error && (
          <div role='alert' className='alert alert-error mt-6'>
            <svg
              xmlns='http://www.w3.org/2000/svg'
              className='stroke-current shrink-0 h-6 w-6'
              fill='none'
              viewBox='0 0 24 24'
            >
              <path
                strokeLinecap='round'
                strokeLinejoin='round'
                strokeWidth='2'
                d='M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z'
              />
            </svg>
            <span>{error}</span>
          </div>
        )}

        {results.length > 0 && (
          <div className='mt-8 animate-in fade-in slide-in-from-bottom-4 duration-500'>
            <h3 className='mb-4 text-lg font-semibold flex items-center gap-2'>
              <svg
                xmlns='http://www.w3.org/2000/svg'
                width='20'
                height='20'
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
                strokeLinecap='round'
                strokeLinejoin='round'
                className='text-success'
              >
                <polyline points='20 6 9 17 4 12' />
              </svg>
              Upload Results
            </h3>
            <div className='flex flex-col gap-4'>
              {results.map((result, idx) => (
                <div
                  key={idx}
                  className={`card border overflow-hidden ${
                    result.success ? 'border-success/20 bg-base-100' : 'border-error/20 bg-error/5'
                  }`}
                >
                  <div className={`p-4 border-b ${result.success ? 'border-success/10' : 'border-error/10'}`}>
                    <div className='flex items-center justify-between'>
                      <div className='font-medium flex items-center gap-2'>
                        {result.success ? (
                          <div className='h-6 w-6 rounded-full bg-success/20 text-success flex items-center justify-center'>
                            ✓
                          </div>
                        ) : (
                          <div className='h-6 w-6 rounded-full bg-error/20 text-error flex items-center justify-center'>
                            ✗
                          </div>
                        )}
                        {result.file}
                      </div>
                      {result.success && <span className='text-xs text-success font-medium'>Completed</span>}
                    </div>
                  </div>

                  {result.success && result.data ? (
                    <div className='p-4 bg-base-200/50'>
                      <div className='flex flex-col gap-3'>
                        <label className='text-xs font-medium text-base-content/70 uppercase tracking-wider'>
                          Embed Code
                        </label>
                        <div className='relative group'>
                          <div className='absolute right-2 top-2 opacity-0 group-hover:opacity-100 transition-opacity'>
                            <Button
                              size='sm'
                              variant='secondary'
                              className='h-8 px-3 text-xs'
                              onClick={() =>
                                copyToClipboard(
                                  `<iframe src="${window.location.origin}${result.data?.player_url}" width="100%" height="100%" frameborder="0" allowfullscreen></iframe>`,
                                  idx.toString()
                                )
                              }
                            >
                              {copiedId === idx.toString() ? 'Copied!' : 'Copy Code'}
                            </Button>
                          </div>
                          <pre className='p-4 rounded-lg bg-base-100 border border-base-300 overflow-x-auto text-sm font-mono'>
                            {`<iframe src="${window.location.origin}${result.data.player_url}" width="100%" height="100%" frameborder="0" allowfullscreen></iframe>`}
                          </pre>
                        </div>
                        <div className='flex justify-end'>
                          <a
                            href={result.data.player_url}
                            target='_blank'
                            rel='noopener noreferrer'
                            className='link link-primary text-xs flex items-center gap-1'
                          >
                            Open Player Page
                            <svg
                              xmlns='http://www.w3.org/2000/svg'
                              width='12'
                              height='12'
                              viewBox='0 0 24 24'
                              fill='none'
                              stroke='currentColor'
                              strokeWidth='2'
                              strokeLinecap='round'
                              strokeLinejoin='round'
                            >
                              <path d='M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6' />
                              <polyline points='15 3 21 3 21 9' />
                              <line x1='10' x2='21' y1='14' y2='3' />
                            </svg>
                          </a>
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div className='p-4 text-sm text-error'>Error: {result.error}</div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
