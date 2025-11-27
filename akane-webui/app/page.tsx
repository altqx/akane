'use client'

import { useRef, FormEvent, useState } from 'react'
import Navbar from '@/components/Navbar'
import Button from '@/components/Button'
import ProcessingQueues from '@/components/ProcessingQueues'
import { useUpload, UploadItem } from '@/context/UploadContext'
import { formatFileSize, formatUploadSpeed } from '@/utils/format'

function UploadItemCard({ item, onRemove }: { item: UploadItem; onRemove: (id: string) => void }) {
  const getStatusBadge = () => {
    switch (item.status) {
      case 'pending':
        return <span className='badge badge-ghost badge-sm'>Pending</span>
      case 'uploading':
        return <span className='badge badge-primary badge-sm'>Uploading</span>
      case 'queued':
        return <span className='badge badge-success badge-sm'>Queued</span>
      case 'error':
        return <span className='badge badge-error badge-sm'>Error</span>
    }
  }

  return (
    <div className={`bg-base-200 rounded-lg p-3 ${item.status === 'error' ? 'border border-error/30' : ''}`}>
      <div className='flex items-center justify-between mb-2'>
        <div className='flex items-center gap-2 flex-1 min-w-0'>
          {item.status === 'uploading' && <span className='loading loading-spinner loading-xs'></span>}
          {item.status === 'queued' && (
            <svg
              xmlns='http://www.w3.org/2000/svg'
              width='14'
              height='14'
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
          )}
          {item.status === 'error' && (
            <svg
              xmlns='http://www.w3.org/2000/svg'
              width='14'
              height='14'
              viewBox='0 0 24 24'
              fill='none'
              stroke='currentColor'
              strokeWidth='2'
              strokeLinecap='round'
              strokeLinejoin='round'
              className='text-error'
            >
              <circle cx='12' cy='12' r='10' />
              <line x1='15' x2='9' y1='9' y2='15' />
              <line x1='9' x2='15' y1='9' y2='15' />
            </svg>
          )}
          <span className='font-medium text-sm truncate' title={item.file.name}>
            {item.file.name}
          </span>
          {getStatusBadge()}
        </div>
        {(item.status === 'queued' || item.status === 'error') && (
          <button className='btn btn-ghost btn-xs' onClick={() => onRemove(item.id)}>
            ✕
          </button>
        )}
      </div>
      {item.status === 'uploading' && (
        <div>
          <progress className='progress progress-primary w-full h-2' value={item.progress} max='100'></progress>
          <div className='flex justify-between text-xs text-base-content/70 mt-1'>
            <span>{item.progress}% uploaded</span>
            {item.speed > 0 && <span>{formatUploadSpeed(item.speed)}</span>}
          </div>
        </div>
      )}
      {item.status === 'error' && item.error && <div className='text-xs text-error mt-1'>{item.error}</div>}
      {item.status === 'queued' && <div className='text-xs text-success mt-1'>Added to processing queue</div>}
    </div>
  )
}

export default function Home() {
  const {
    files,
    addFiles,
    updateFileMetadata,
    removeFile,
    isUploading,
    uploadItems,
    error,
    setError,
    startUpload,
    clearUploads,
    cancelUpload,
    removeUploadItem
  } = useUpload()

  const fileInputRef = useRef<HTMLInputElement>(null)
  const [isDragging, setIsDragging] = useState(false)

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) {
      const newFiles = Array.from(e.target.files)
      addFiles(newFiles)
      setError(null)
      // Reset input so same file can be selected again
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
    if (e.dataTransfer.files) {
      const newFiles = Array.from(e.dataTransfer.files).filter(
        (f) => f.type.startsWith('video/') || f.name.endsWith('.mkv')
      )
      if (newFiles.length > 0) {
        addFiles(newFiles)
        setError(null)
      }
    }
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    await startUpload()
  }

  const handleClear = () => {
    clearUploads()
    if (fileInputRef.current) fileInputRef.current.value = ''
  }

  // Separate upload items by status
  const activeUploads = uploadItems.filter((i) => i.status === 'uploading' || i.status === 'pending')
  const queuedUploads = uploadItems.filter((i) => i.status === 'queued')
  const errorUploads = uploadItems.filter((i) => i.status === 'error')

  return (
    <div className='min-h-screen bg-base-200 p-10 font-sans'>
      <div className='mx-auto max-w-3xl'>
        <div className='flex justify-between items-center mb-8'>
          <div>
            <h1 className='text-3xl font-bold tracking-tight'>Upload Video</h1>
            <p className='text-base-content/70 mt-1'>Upload videos to the processing queue.</p>
          </div>
        </div>
        <Navbar />

        <ProcessingQueues />

        {/* Current Upload Progress */}
        {uploadItems.length > 0 && (
          <div className='card bg-base-100 shadow-xl mb-6'>
            <div className='card-body p-4'>
              <h3 className='card-title text-base flex items-center gap-2'>
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
                >
                  <path d='M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4' />
                  <polyline points='17 8 12 3 7 8' />
                  <line x1='12' x2='12' y1='3' y2='15' />
                </svg>
                Upload Progress
                {activeUploads.length > 0 && (
                  <span className='badge badge-primary badge-sm'>{activeUploads.length} uploading</span>
                )}
              </h3>

              <div className='space-y-2 mt-2'>
                {activeUploads.map((item) => (
                  <UploadItemCard key={item.id} item={item} onRemove={removeUploadItem} />
                ))}
                {queuedUploads.length > 0 && (
                  <div className='text-xs font-semibold text-base-content/70 uppercase tracking-wider mt-3 mb-1'>
                    Successfully Queued ({queuedUploads.length})
                  </div>
                )}
                {queuedUploads.map((item) => (
                  <UploadItemCard key={item.id} item={item} onRemove={removeUploadItem} />
                ))}
                {errorUploads.length > 0 && (
                  <div className='text-xs font-semibold text-error uppercase tracking-wider mt-3 mb-1'>
                    Failed ({errorUploads.length})
                  </div>
                )}
                {errorUploads.map((item) => (
                  <UploadItemCard key={item.id} item={item} onRemove={removeUploadItem} />
                ))}
              </div>

              {isUploading && (
                <div className='flex justify-end mt-2'>
                  <Button size='sm' variant='secondary' onClick={cancelUpload}>
                    Cancel Upload
                  </Button>
                </div>
              )}
            </div>
          </div>
        )}

        <div className='card bg-base-100 shadow-xl'>
          <div className='card-body'>
            <form onSubmit={handleSubmit} className='flex flex-col gap-6'>
              <div className='form-control w-full'>
                <div className='label'>
                  <span className='label-text'>Video Files *</span>
                </div>
                <div
                  className={`relative border-2 border-dashed rounded-lg transition-colors ${
                    isDragging ? 'border-primary bg-primary/5' : 'border-base-300'
                  }`}
                  onDragOver={(e) => {
                    e.preventDefault()
                    setIsDragging(true)
                  }}
                  onDragLeave={() => setIsDragging(false)}
                  onDrop={handleDrop}
                >
                  <input
                    ref={fileInputRef}
                    type='file'
                    id='fileInput'
                    accept='video/*,.mkv'
                    multiple
                    onChange={handleFileChange}
                    className='absolute inset-0 w-full h-full opacity-0 cursor-pointer'
                  />
                  <div className='flex flex-col items-center justify-center py-8 text-base-content/50'>
                    <svg
                      xmlns='http://www.w3.org/2000/svg'
                      width='32'
                      height='32'
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
                    <span className='text-sm font-medium'>Drop files here or click to select</span>
                    <span className='text-xs mt-1'>You can upload while previous files are processing</span>
                  </div>
                </div>
              </div>

              {files.length > 0 && (
                <div className='bg-base-200 rounded-box p-4'>
                  <div className='text-xs font-semibold text-base-content/70 uppercase tracking-wider mb-3'>
                    Ready to Upload ({files.length})
                  </div>
                  <div className='flex flex-col gap-4'>
                    {files.map((fileItem) => (
                      <div key={fileItem.id} className='bg-base-100 rounded-lg p-3'>
                        <div className='flex items-start gap-3'>
                          <div className='h-10 w-10 rounded bg-primary/10 flex items-center justify-center text-primary flex-shrink-0 mt-1'>
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
                            >
                              <path d='m22 8-6 4 6 4V8Z' />
                              <rect width='14' height='12' x='2' y='6' rx='2' ry='2' />
                            </svg>
                          </div>
                          <div className='flex-1 min-w-0'>
                            <div className='flex items-center justify-between mb-2'>
                              <div className='text-xs text-base-content/50 truncate' title={fileItem.file.name}>
                                {fileItem.file.name} • {formatFileSize(fileItem.file.size)}
                              </div>
                              <button
                                type='button'
                                className='btn btn-ghost btn-xs'
                                onClick={() => removeFile(fileItem.id)}
                              >
                                ✕
                              </button>
                            </div>
                            <input
                              type='text'
                              placeholder='Video name'
                              value={fileItem.name}
                              onChange={(e) => updateFileMetadata(fileItem.id, { name: e.target.value })}
                              className='input input-bordered input-sm w-full mb-2'
                            />
                            <input
                              type='text'
                              placeholder='Tags (comma separated)'
                              value={fileItem.tags}
                              onChange={(e) => updateFileMetadata(fileItem.id, { tags: e.target.value })}
                              className='input input-bordered input-sm w-full'
                            />
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              <div className='flex gap-3 pt-2'>
                <Button type='submit' disabled={files.length === 0} className='flex-1'>
                  {isUploading ? (
                    <span className='flex items-center gap-2'>
                      <span className='loading loading-spinner loading-sm'></span>
                      Uploading...
                    </span>
                  ) : (
                    `Upload ${files.length > 0 ? `(${files.length} file${files.length > 1 ? 's' : ''})` : ''}`
                  )}
                </Button>
                <Button type='button' variant='secondary' onClick={handleClear} className='flex-1'>
                  Clear
                </Button>
              </div>
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
      </div>
    </div>
  )
}
