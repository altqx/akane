'use client'

import React, { createContext, useContext, useState, useCallback, useRef } from 'react'

export interface UploadResult {
  file: string
  success: boolean
  data?: {
    player_url: string
    upload_id: string
  }
  error?: string
}

export interface ProgressData {
  percentage: number
  stage: string
  current_chunk: number
  total_chunks: number
  details?: string
  status?: string
  result?: {
    player_url: string
    upload_id: string
  }
  error?: string
}

interface UploadContextType {
  files: File[]
  setFiles: (files: File[]) => void
  tags: string
  setTags: (tags: string) => void
  isUploading: boolean
  progress: ProgressData | null
  results: UploadResult[]
  error: string | null
  setError: (error: string | null) => void
  uploadStatus: string
  startUpload: () => Promise<void>
  clearUploads: () => void
  cancelUpload: () => void
}

const UploadContext = createContext<UploadContextType | undefined>(undefined)

// Constants for better maintainability
const PROGRESS_TIMEOUT_MS = 60000 // 60 seconds
const SSE_CLOSE_GRACE_PERIOD_MS = 100 // 100ms grace period to process completion message
const CHUNK_SIZE = 50 * 1024 * 1024 // 50MB chunks (under Cloudflare's 100MB limit)

// Fallback UUID generator for browsers that don't support crypto.randomUUID
function generateUUID(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  // Fallback implementation using crypto.getRandomValues
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    return '10000000-1000-4000-8000-100000000000'.replace(/[018]/g, (c) =>
      (+c ^ (crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (+c / 4)))).toString(16)
    )
  }
  // Last resort fallback using Math.random (less secure but works everywhere)
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0
    const v = c === 'x' ? r : (r & 0x3) | 0x8
    return v.toString(16)
  })
}

// Get API base URL - in development with Next.js dev server, go directly to backend
// to bypass Next.js proxy body size limits for large file uploads
function getApiBaseUrl(): string {
  if (typeof window === 'undefined') return ''
  
  // Check if we're in development mode (Next.js dev server on port 3001)
  // or accessing via basePath which indicates production
  const isDev = window.location.port === '3001'
  
  if (isDev) {
    // Bypass Next.js proxy for large uploads - go directly to Rust backend
    return 'http://localhost:3000'
  }
  
  // In production (static export served by backend), use relative paths
  return ''
}

export function UploadProvider({ children }: { children: React.ReactNode }) {
  const [files, setFiles] = useState<File[]>([])
  const [tags, setTags] = useState('')
  const [isUploading, setIsUploading] = useState(false)
  const [progress, setProgress] = useState<ProgressData | null>(null)
  const [results, setResults] = useState<UploadResult[]>([])
  const [error, setError] = useState<string | null>(null)
  const [uploadStatus, setUploadStatus] = useState<string>('')

  // Refs for cleanup and cancellation
  const abortControllerRef = useRef<AbortController | null>(null)
  const eventSourceRef = useRef<EventSource | null>(null)

  // Helper function to create progress stream listener with race condition prevention
  const createProgressListener = useCallback(
    (uploadId: string, token: string | null): Promise<{ player_url: string; upload_id: string }> => {
      return new Promise((resolve, reject) => {
        const apiBase = getApiBaseUrl()
        // EventSource doesn't support custom headers, so pass token as query param
        const url = token ? `${apiBase}/api/progress/${uploadId}?token=${encodeURIComponent(token)}` : `${apiBase}/api/progress/${uploadId}`
        console.log('[Upload] Progress EventSource URL:', url)
        const eventSource = new EventSource(url)
        eventSourceRef.current = eventSource

        let timeoutId: NodeJS.Timeout
        let latestProgress: ProgressData | null = null
        let isResolved = false // Prevent multiple resolves/rejects
        let hasCompleted = false // Track if we've received completion status

        const cleanup = () => {
          if (timeoutId) clearTimeout(timeoutId)
          if (eventSource.readyState !== EventSource.CLOSED) {
            eventSource.close()
          }
          eventSourceRef.current = null
        }

        const safeResolve = (result: { player_url: string; upload_id: string }) => {
          if (!isResolved) {
            isResolved = true
            cleanup()
            resolve(result)
          }
        }

        const safeReject = (error: Error) => {
          if (!isResolved) {
            isResolved = true
            cleanup()
            reject(error)
          }
        }

        // Set a timeout to reject if we don't get any messages
        const resetTimeout = () => {
          if (timeoutId) clearTimeout(timeoutId)
          timeoutId = setTimeout(() => {
            safeReject(new Error('Connection timed out waiting for progress updates'))
          }, PROGRESS_TIMEOUT_MS)
        }

        resetTimeout()

        eventSource.onmessage = (event) => {
          resetTimeout()
          try {
            const data: ProgressData = JSON.parse(event.data)
            latestProgress = data
            setProgress(data)

            if (data.status === 'completed' && data.result) {
              // Mark as completed immediately to prevent race condition with onerror
              hasCompleted = true
              // Add a tiny delay to ensure the progress update is rendered
              setTimeout(() => {
                safeResolve(data.result!)
              }, SSE_CLOSE_GRACE_PERIOD_MS)
            } else if (data.status === 'failed') {
              hasCompleted = true
              safeReject(new Error(data.error || 'Processing failed'))
            }
          } catch (e) {
            console.error('Failed to parse progress data:', e)
            // Don't fail on parse errors, just log them
          }
        }

        eventSource.onerror = () => {
          // Only handle error if connection is actually closed
          if (eventSource.readyState === EventSource.CLOSED) {
            // If we've already marked as completed, this is just the expected stream closure
            if (hasCompleted) {
              // Do nothing - we already resolved or will resolve shortly
              return
            }

            // Check if we have a completed status from the last message (backup check)
            if (latestProgress?.status === 'completed' && latestProgress?.result) {
              // This is expected - the stream closed after sending completion
              hasCompleted = true
              safeResolve(latestProgress.result)
            } else if (!isResolved) {
              // Unexpected closure without completion
              safeReject(new Error('Connection to progress stream closed unexpectedly'))
            }
          }
          // For CONNECTING state, browser will auto-retry
        }
      })
    },
    []
  )

  // Helper function to upload a single chunk
  const uploadChunk = useCallback(
    async (
      chunk: Blob,
      uploadId: string,
      chunkIndex: number,
      totalChunks: number,
      fileName: string,
      token: string | null
    ): Promise<void> => {
      return new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        const formData = new FormData()

        formData.append('chunk', chunk)
        formData.append('chunk_index', chunkIndex.toString())
        formData.append('total_chunks', totalChunks.toString())
        formData.append('file_name', fileName)

        xhr.addEventListener('load', () => {
          if (xhr.status >= 200 && xhr.status < 300) {
            resolve()
          } else {
            let errorMsg = 'Chunk upload failed'
            try {
              const response = JSON.parse(xhr.responseText)
              errorMsg = response.error || response.message || errorMsg
            } catch {
              errorMsg = xhr.responseText || errorMsg
            }
            reject(new Error(errorMsg))
          }
        })

        xhr.addEventListener('error', () => reject(new Error('Network error during chunk upload')))
        xhr.addEventListener('abort', () => reject(new Error('Chunk upload aborted')))

        const apiBase = getApiBaseUrl()
        xhr.open('POST', `${apiBase}/api/upload/chunk`)
        xhr.setRequestHeader('X-Upload-ID', uploadId)
        if (token) {
          xhr.setRequestHeader('Authorization', `Bearer ${token}`)
        }
        xhr.send(formData)

        // Store xhr for potential cancellation
        abortControllerRef.current = {
          abort: () => xhr.abort()
        } as AbortController
      })
    },
    []
  )

  // Helper function to finalize chunked upload
  const finalizeUpload = useCallback(
    async (uploadId: string, fileName: string, fileTags: string, token: string | null): Promise<void> => {
      const apiBase = getApiBaseUrl()
      const response = await fetch(`${apiBase}/api/upload/finalize`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Upload-ID': uploadId,
          ...(token ? { Authorization: `Bearer ${token}` } : {})
        },
        body: JSON.stringify({
          name: fileName.replace(/\.[^/.]+$/, ''),
          tags: fileTags.trim() || undefined
        })
      })

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}))
        throw new Error(errorData.error || errorData.message || 'Failed to finalize upload')
      }
    },
    []
  )

  // Helper function to upload file via chunked upload (for Cloudflare Tunnels compatibility)
  const uploadFileChunked = useCallback(
    async (file: File, uploadId: string, token: string | null): Promise<void> => {
      const totalChunks = Math.ceil(file.size / CHUNK_SIZE)
      console.log(`[Upload] Chunked upload: ${totalChunks} chunks of ${CHUNK_SIZE / 1024 / 1024}MB each`)

      for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
        const start = chunkIndex * CHUNK_SIZE
        const end = Math.min(start + CHUNK_SIZE, file.size)
        const chunk = file.slice(start, end)

        setProgress({
          stage: 'Uploading to server',
          percentage: Math.round((chunkIndex / totalChunks) * 100),
          current_chunk: chunkIndex + 1,
          total_chunks: totalChunks,
          details: `Uploading chunk ${chunkIndex + 1} of ${totalChunks} (${(start / 1024 / 1024).toFixed(1)}MB - ${(end / 1024 / 1024).toFixed(1)}MB)`,
          status: 'uploading'
        })

        console.log(`[Upload] Uploading chunk ${chunkIndex + 1}/${totalChunks}`)
        await uploadChunk(chunk, uploadId, chunkIndex, totalChunks, file.name, token)
      }

      // Finalize the upload
      setProgress({
        stage: 'Finalizing upload',
        percentage: 100,
        current_chunk: totalChunks,
        total_chunks: totalChunks,
        details: 'Assembling file on server...',
        status: 'uploading'
      })

      console.log('[Upload] All chunks uploaded, finalizing...')
      await finalizeUpload(uploadId, file.name, tags, token)
      console.log('[Upload] Upload finalized')
    },
    [tags, uploadChunk, finalizeUpload]
  )

  // Helper function to upload file via XMLHttpRequest (single request for small files)
  const uploadFile = useCallback(
    (file: File, uploadId: string, token: string | null): Promise<void> => {
      return new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        const formData = new FormData()

        formData.append('file', file)
        formData.append('name', file.name.replace(/\.[^/.]+$/, ''))
        if (tags.trim()) {
          formData.append('tags', tags.trim())
        }

        // Track upload start
        xhr.upload.addEventListener('loadstart', () => {
          console.log('[Upload] XHR upload started')
        })

        // Track upload progress
        xhr.upload.addEventListener('progress', (event) => {
          console.log('[Upload] Progress event:', event.loaded, '/', event.total, 'lengthComputable:', event.lengthComputable)
          if (event.lengthComputable) {
            const percentComplete = Math.round((event.loaded / event.total) * 100)
            setProgress((prev) => {
              // Only update if we're still in the uploading stage
              if (!prev || prev.stage === 'Uploading to server' || prev.stage === 'Initializing upload') {
                return {
                  stage: 'Uploading to server',
                  percentage: percentComplete,
                  current_chunk: 0,
                  total_chunks: 1,
                  details: `Uploaded ${(event.loaded / 1024 / 1024).toFixed(2)} MB of ${(event.total / 1024 / 1024).toFixed(2)} MB`,
                  status: 'uploading'
                }
              }
              return prev
            })
          }
        })

        xhr.addEventListener('load', () => {
          console.log('[Upload] XHR load complete, status:', xhr.status)
          if (xhr.status >= 200 && xhr.status < 300) {
            resolve()
          } else {
            let errorMsg = 'Upload failed'
            try {
              const response = JSON.parse(xhr.responseText)
              errorMsg = response.error || response.message || errorMsg
            } catch {
              errorMsg = xhr.responseText || errorMsg
            }
            reject(new Error(errorMsg))
          }
        })

        xhr.addEventListener('error', (e) => {
          console.error('[Upload] XHR error event:', e)
          reject(new Error('Network error during upload'))
        })
        xhr.addEventListener('abort', () => reject(new Error('Upload aborted')))

        const apiBase = getApiBaseUrl()
        const uploadUrl = `${apiBase}/api/upload`
        console.log('[Upload] XHR Upload URL:', uploadUrl)
        xhr.open('POST', uploadUrl)
        xhr.setRequestHeader('X-Upload-ID', uploadId)
        if (token) {
          xhr.setRequestHeader('Authorization', `Bearer ${token}`)
        }
        xhr.send(formData)

        // Store xhr for potential cancellation
        abortControllerRef.current = {
          abort: () => xhr.abort()
        } as AbortController
      })
    },
    [tags]
  )

  const startUpload = useCallback(async () => {
    console.log('[Upload] startUpload called, files:', files.length)
    
    if (files.length === 0) {
      setError('Please select at least one video file.')
      return
    }

    setIsUploading(true)
    setResults([])
    setError(null)

    const newResults: UploadResult[] = []
    const filesToUpload = [...files]

    for (let i = 0; i < filesToUpload.length; i++) {
      const file = filesToUpload[i]
      console.log(`[Upload] Processing file ${i + 1}/${filesToUpload.length}: ${file.name}`)
      setUploadStatus(`Processing ${i + 1} of ${filesToUpload.length}: ${file.name}`)
      setProgress(null)

      const uploadId = generateUUID()
      const token = localStorage.getItem('admin_token')
      console.log(`[Upload] Upload ID: ${uploadId}, Token exists: ${!!token}`)

      try {
        // Start upload and progress listener in parallel
        // The upload will initialize progress on the server, and the progress listener
        // will wait up to 60 seconds for the progress to appear
        console.log('[Upload] Starting file upload...')
        
        // Use chunked upload for files > 50MB (Cloudflare Tunnel limit is 100MB)
        const useChunkedUpload = file.size > CHUNK_SIZE
        console.log(`[Upload] File size: ${(file.size / 1024 / 1024).toFixed(2)}MB, using ${useChunkedUpload ? 'chunked' : 'single'} upload`)
        
        const uploadPromise = useChunkedUpload 
          ? uploadFileChunked(file, uploadId, token)
          : uploadFile(file, uploadId, token)
        
        // Small delay to ensure the upload request reaches the server first
        // This gives the server time to initialize the progress entry
        await new Promise(resolve => setTimeout(resolve, 100))
        
        console.log('[Upload] Starting progress listener...')
        const processingPromise = createProgressListener(uploadId, token)

        // Wait for upload to complete (file transfer to server)
        console.log('[Upload] Waiting for upload to complete...')
        await uploadPromise
        console.log('[Upload] Upload completed, waiting for processing...')

        // Wait for processing to complete (encoding + R2 upload)
        const data = await processingPromise
        console.log('[Upload] Processing completed:', data)

        const result: UploadResult = {
          file: file.name,
          success: true,
          data: data
        }

        newResults.push(result)
        setResults([...newResults])
      } catch (err: unknown) {
        console.error('[Upload] Error:', err)
        const errorMessage = err instanceof Error ? err.message : String(err)
        const result: UploadResult = {
          file: file.name,
          success: false,
          error: errorMessage
        }
        newResults.push(result)
        setResults([...newResults])
      }
    }

    console.log('[Upload] All uploads finished')
    setIsUploading(false)
    setUploadStatus('')
    setProgress(null)
    setFiles([])

    // Cleanup refs
    abortControllerRef.current = null
    eventSourceRef.current = null
  }, [files, createProgressListener, uploadFile, uploadFileChunked])

  const cancelUpload = useCallback(() => {
    // Cancel ongoing XHR request
    if (abortControllerRef.current) {
      abortControllerRef.current.abort()
      abortControllerRef.current = null
    }

    // Close EventSource connection
    if (eventSourceRef.current) {
      eventSourceRef.current.close()
      eventSourceRef.current = null
    }

    setIsUploading(false)
    setUploadStatus('')
    setProgress(null)
    setError('Upload cancelled by user')
  }, [])

  const clearUploads = useCallback(() => {
    setFiles([])
    setResults([])
    setError(null)
    setProgress(null)
    setUploadStatus('')
  }, [])

  return (
    <UploadContext.Provider
      value={{
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
        clearUploads,
        cancelUpload
      }}
    >
      {children}
    </UploadContext.Provider>
  )
}

export function useUpload() {
  const context = useContext(UploadContext)
  if (context === undefined) {
    throw new Error('useUpload must be used within an UploadProvider')
  }
  return context
}
