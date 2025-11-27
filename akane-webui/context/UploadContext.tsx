'use client'

import React, { createContext, useContext, useState, useCallback, useRef } from 'react'

export interface FileWithMetadata {
  id: string
  file: File
  name: string // custom video name
  tags: string // custom tags for this file
}

export interface UploadItem {
  id: string
  file: File
  name: string
  tags: string
  status: 'pending' | 'uploading' | 'queued' | 'error'
  progress: number
  speed: number // bytes per second
  error?: string
}

interface UploadContextType {
  files: FileWithMetadata[]
  setFiles: (files: FileWithMetadata[]) => void
  addFiles: (newFiles: File[]) => void
  updateFileMetadata: (id: string, updates: Partial<Pick<FileWithMetadata, 'name' | 'tags'>>) => void
  removeFile: (id: string) => void
  isUploading: boolean
  uploadItems: UploadItem[]
  error: string | null
  setError: (error: string | null) => void
  startUpload: () => Promise<void>
  clearUploads: () => void
  cancelUpload: () => void
  removeUploadItem: (id: string) => void
}

const UploadContext = createContext<UploadContextType | undefined>(undefined)

// Constants
const CHUNK_SIZE = 50 * 1024 * 1024 // 50MB chunks (under Cloudflare's 100MB limit)

// Fallback UUID generator for browsers that don't support crypto.randomUUID
function generateUUID(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    return '10000000-1000-4000-8000-100000000000'.replace(/[018]/g, (c) =>
      (+c ^ (crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (+c / 4)))).toString(16)
    )
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0
    const v = c === 'x' ? r : (r & 0x3) | 0x8
    return v.toString(16)
  })
}

// Get API base URL
function getApiBaseUrl(): string {
  if (typeof window === 'undefined') return ''
  const isDev = window.location.port === '3001'
  if (isDev) {
    return 'http://localhost:3000'
  }
  return ''
}

export function UploadProvider({ children }: { children: React.ReactNode }) {
  const [files, setFiles] = useState<FileWithMetadata[]>([])
  const [isUploading, setIsUploading] = useState(false)
  const [uploadItems, setUploadItems] = useState<UploadItem[]>([])
  const [error, setError] = useState<string | null>(null)

  const abortControllerRef = useRef<AbortController | null>(null)

  // Add new files with default metadata
  const addFiles = useCallback((newFiles: File[]) => {
    const filesWithMetadata: FileWithMetadata[] = newFiles.map((file) => ({
      id: generateUUID(),
      file,
      name: file.name.replace(/\.[^/.]+$/, ''), // default to filename without extension
      tags: ''
    }))
    setFiles((prev) => [...prev, ...filesWithMetadata])
  }, [])

  // Update file metadata (name or tags)
  const updateFileMetadata = useCallback((id: string, updates: Partial<Pick<FileWithMetadata, 'name' | 'tags'>>) => {
    setFiles((prev) => prev.map((f) => (f.id === id ? { ...f, ...updates } : f)))
  }, [])

  // Remove a file from the list
  const removeFile = useCallback((id: string) => {
    setFiles((prev) => prev.filter((f) => f.id !== id))
  }, [])

  // Update a single upload item's state
  const updateUploadItem = useCallback((id: string, updates: Partial<UploadItem>) => {
    setUploadItems((prev) => prev.map((item) => (item.id === id ? { ...item, ...updates } : item)))
  }, [])

  // Remove an upload item from the list
  const removeUploadItem = useCallback((id: string) => {
    setUploadItems((prev) => prev.filter((item) => item.id !== id))
  }, [])

  // Upload a single chunk
  const uploadChunk = useCallback(
    async (
      chunk: Blob,
      uploadId: string,
      chunkIndex: number,
      totalChunks: number,
      fileName: string,
      token: string | null,
      signal?: AbortSignal
    ): Promise<void> => {
      const formData = new FormData()
      formData.append('chunk', chunk)
      formData.append('chunk_index', chunkIndex.toString())
      formData.append('total_chunks', totalChunks.toString())
      formData.append('file_name', fileName)

      const apiBase = getApiBaseUrl()
      const response = await fetch(`${apiBase}/api/upload/chunk`, {
        method: 'POST',
        headers: {
          'X-Upload-ID': uploadId,
          ...(token ? { Authorization: `Bearer ${token}` } : {})
        },
        body: formData,
        signal
      })

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}))
        throw new Error(errorData.error || errorData.message || 'Chunk upload failed')
      }
    },
    []
  )

  // Finalize a chunked upload
  const finalizeUpload = useCallback(
    async (
      uploadId: string,
      videoName: string,
      fileTags: string,
      token: string | null,
      signal?: AbortSignal
    ): Promise<void> => {
      const apiBase = getApiBaseUrl()
      const response = await fetch(`${apiBase}/api/upload/finalize`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Upload-ID': uploadId,
          ...(token ? { Authorization: `Bearer ${token}` } : {})
        },
        body: JSON.stringify({
          name: videoName,
          tags: fileTags.trim() || undefined
        }),
        signal
      })

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}))
        throw new Error(errorData.error || errorData.message || 'Failed to finalize upload')
      }
    },
    []
  )

  // Upload file in chunks (for large files > 50MB)
  const uploadFileChunked = useCallback(
    async (
      file: File,
      uploadId: string,
      token: string | null,
      videoName: string,
      fileTags: string,
      onProgress: (progress: number, bytesUploaded: number) => void,
      signal?: AbortSignal
    ): Promise<void> => {
      const totalChunks = Math.ceil(file.size / CHUNK_SIZE)
      let totalBytesUploaded = 0

      for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
        if (signal?.aborted) throw new Error('Upload cancelled')

        const start = chunkIndex * CHUNK_SIZE
        const end = Math.min(start + CHUNK_SIZE, file.size)
        const chunk = file.slice(start, end)

        await uploadChunk(chunk, uploadId, chunkIndex, totalChunks, file.name, token, signal)
        totalBytesUploaded = end
        onProgress(Math.round(((chunkIndex + 1) / totalChunks) * 90), totalBytesUploaded) // 0-90% for chunks
      }

      // Finalize
      onProgress(95, file.size)
      await finalizeUpload(uploadId, videoName, fileTags, token, signal)
      onProgress(100, file.size)
    },
    [uploadChunk, finalizeUpload]
  )

  // Upload file in a single request (for small files <= 50MB)
  const uploadFileSingle = useCallback(
    async (
      file: File,
      uploadId: string,
      token: string | null,
      videoName: string,
      fileTags: string,
      onProgress: (progress: number, bytesUploaded: number) => void,
      signal?: AbortSignal
    ): Promise<void> => {
      return new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        const formData = new FormData()

        formData.append('file', file)
        formData.append('name', videoName)
        if (fileTags.trim()) {
          formData.append('tags', fileTags.trim())
        }

        xhr.upload.addEventListener('progress', (event) => {
          if (event.lengthComputable) {
            onProgress(Math.round((event.loaded / event.total) * 100), event.loaded)
          }
        })

        xhr.addEventListener('load', () => {
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

        xhr.addEventListener('error', () => reject(new Error('Network error during upload')))
        xhr.addEventListener('abort', () => reject(new Error('Upload cancelled')))

        // Handle abort signal
        if (signal) {
          signal.addEventListener('abort', () => xhr.abort())
        }

        const apiBase = getApiBaseUrl()
        xhr.open('POST', `${apiBase}/api/upload`)
        xhr.setRequestHeader('X-Upload-ID', uploadId)
        if (token) {
          xhr.setRequestHeader('Authorization', `Bearer ${token}`)
        }
        xhr.send(formData)
      })
    },
    []
  )

  // Upload a single file (decides between chunked and single based on size)
  const uploadSingleFile = useCallback(
    async (item: UploadItem, token: string | null, signal?: AbortSignal): Promise<void> => {
      // Track progress with 1-second throttling
      let lastUpdateTime = Date.now()
      let lastBytesUploaded = 0
      let latestProgress = 0
      let latestBytesUploaded = 0
      let updateIntervalId: ReturnType<typeof setInterval> | null = null

      const onProgress = (progress: number, bytesUploaded: number) => {
        latestProgress = progress
        latestBytesUploaded = bytesUploaded
      }

      // Start interval to update UI every second
      const startProgressInterval = () => {
        updateIntervalId = setInterval(() => {
          const now = Date.now()
          const timeDelta = (now - lastUpdateTime) / 1000 // seconds
          const bytesDelta = latestBytesUploaded - lastBytesUploaded
          const speed = timeDelta > 0 ? bytesDelta / timeDelta : 0

          updateUploadItem(item.id, {
            progress: latestProgress,
            speed: speed
          })

          lastUpdateTime = now
          lastBytesUploaded = latestBytesUploaded
        }, 1000)
      }

      const stopProgressInterval = () => {
        if (updateIntervalId) {
          clearInterval(updateIntervalId)
          updateIntervalId = null
        }
      }

      updateUploadItem(item.id, { status: 'uploading', progress: 0, speed: 0 })
      startProgressInterval()

      try {
        if (item.file.size > CHUNK_SIZE) {
          await uploadFileChunked(item.file, item.id, token, item.name, item.tags, onProgress, signal)
        } else {
          await uploadFileSingle(item.file, item.id, token, item.name, item.tags, onProgress, signal)
        }

        stopProgressInterval()
        updateUploadItem(item.id, { status: 'queued', progress: 100, speed: 0 })
      } catch (err) {
        stopProgressInterval()
        const errorMessage = err instanceof Error ? err.message : String(err)
        updateUploadItem(item.id, { status: 'error', error: errorMessage, speed: 0 })
        throw err
      }
    },
    [updateUploadItem, uploadFileChunked, uploadFileSingle]
  )

  const startUpload = useCallback(async () => {
    if (files.length === 0) {
      setError('Please select at least one video file.')
      return
    }

    setError(null)
    setIsUploading(true)

    const abortController = new AbortController()
    abortControllerRef.current = abortController

    const token = localStorage.getItem('admin_token')

    // Create upload items for all files with their metadata
    const newItems: UploadItem[] = files.map((f) => ({
      id: generateUUID(),
      file: f.file,
      name: f.name,
      tags: f.tags,
      status: 'pending' as const,
      progress: 0,
      speed: 0
    }))

    setUploadItems((prev) => [...prev, ...newItems])
    setFiles([]) // Clear the file input immediately so user can add more

    // Upload files sequentially (to avoid overwhelming the server)
    for (const item of newItems) {
      if (abortController.signal.aborted) break

      try {
        await uploadSingleFile(item, token, abortController.signal)
      } catch (err) {
        // Error already handled in uploadSingleFile, continue with next file
        console.error(`Failed to upload ${item.file.name}:`, err)
      }
    }

    setIsUploading(false)
    abortControllerRef.current = null
  }, [files, uploadSingleFile])

  const cancelUpload = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort()
      abortControllerRef.current = null
    }
    setIsUploading(false)
    setError('Upload cancelled by user')
  }, [])

  const clearUploads = useCallback(() => {
    setFiles([])
    // Only clear completed/error items, keep uploading ones
    setUploadItems((prev) => prev.filter((item) => item.status === 'uploading'))
    setError(null)
  }, [])

  return (
    <UploadContext.Provider
      value={{
        files,
        setFiles,
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
