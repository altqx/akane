'use client'

import React, { createContext, useContext, useState, useCallback, useRef } from 'react'

export interface UploadItem {
  id: string
  file: File
  status: 'pending' | 'uploading' | 'queued' | 'error'
  progress: number
  error?: string
}

interface UploadContextType {
  files: File[]
  setFiles: (files: File[]) => void
  tags: string
  setTags: (tags: string) => void
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
  const [files, setFiles] = useState<File[]>([])
  const [tags, setTags] = useState('')
  const [isUploading, setIsUploading] = useState(false)
  const [uploadItems, setUploadItems] = useState<UploadItem[]>([])
  const [error, setError] = useState<string | null>(null)

  const abortControllerRef = useRef<AbortController | null>(null)

  // Update a single upload item's state
  const updateUploadItem = useCallback((id: string, updates: Partial<UploadItem>) => {
    setUploadItems(prev => prev.map(item => 
      item.id === id ? { ...item, ...updates } : item
    ))
  }, [])

  // Remove an upload item from the list
  const removeUploadItem = useCallback((id: string) => {
    setUploadItems(prev => prev.filter(item => item.id !== id))
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
    async (uploadId: string, fileName: string, fileTags: string, token: string | null, signal?: AbortSignal): Promise<void> => {
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
    async (file: File, uploadId: string, token: string | null, fileTags: string, onProgress: (progress: number) => void, signal?: AbortSignal): Promise<void> => {
      const totalChunks = Math.ceil(file.size / CHUNK_SIZE)

      for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
        if (signal?.aborted) throw new Error('Upload cancelled')
        
        const start = chunkIndex * CHUNK_SIZE
        const end = Math.min(start + CHUNK_SIZE, file.size)
        const chunk = file.slice(start, end)

        await uploadChunk(chunk, uploadId, chunkIndex, totalChunks, file.name, token, signal)
        onProgress(Math.round(((chunkIndex + 1) / totalChunks) * 90)) // 0-90% for chunks
      }

      // Finalize
      onProgress(95)
      await finalizeUpload(uploadId, file.name, fileTags, token, signal)
      onProgress(100)
    },
    [uploadChunk, finalizeUpload]
  )

  // Upload file in a single request (for small files <= 50MB)
  const uploadFileSingle = useCallback(
    async (file: File, uploadId: string, token: string | null, fileTags: string, onProgress: (progress: number) => void, signal?: AbortSignal): Promise<void> => {
      return new Promise((resolve, reject) => {
        const xhr = new XMLHttpRequest()
        const formData = new FormData()

        formData.append('file', file)
        formData.append('name', file.name.replace(/\.[^/.]+$/, ''))
        if (fileTags.trim()) {
          formData.append('tags', fileTags.trim())
        }

        xhr.upload.addEventListener('progress', (event) => {
          if (event.lengthComputable) {
            onProgress(Math.round((event.loaded / event.total) * 100))
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
    async (item: UploadItem, token: string | null, fileTags: string, signal?: AbortSignal): Promise<void> => {
      const onProgress = (progress: number) => {
        updateUploadItem(item.id, { progress })
      }

      updateUploadItem(item.id, { status: 'uploading', progress: 0 })

      try {
        if (item.file.size > CHUNK_SIZE) {
          await uploadFileChunked(item.file, item.id, token, fileTags, onProgress, signal)
        } else {
          await uploadFileSingle(item.file, item.id, token, fileTags, onProgress, signal)
        }
        
        updateUploadItem(item.id, { status: 'queued', progress: 100 })
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : String(err)
        updateUploadItem(item.id, { status: 'error', error: errorMessage })
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
    const currentTags = tags // Capture tags at upload time

    // Create upload items for all files
    const newItems: UploadItem[] = files.map(file => ({
      id: generateUUID(),
      file,
      status: 'pending' as const,
      progress: 0
    }))

    setUploadItems(prev => [...prev, ...newItems])
    setFiles([]) // Clear the file input immediately so user can add more

    // Upload files sequentially (to avoid overwhelming the server)
    for (const item of newItems) {
      if (abortController.signal.aborted) break
      
      try {
        await uploadSingleFile(item, token, currentTags, abortController.signal)
      } catch (err) {
        // Error already handled in uploadSingleFile, continue with next file
        console.error(`Failed to upload ${item.file.name}:`, err)
      }
    }

    setIsUploading(false)
    abortControllerRef.current = null
  }, [files, tags, uploadSingleFile])

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
    setUploadItems(prev => prev.filter(item => item.status === 'uploading'))
    setError(null)
  }, [])

  return (
    <UploadContext.Provider
      value={{
        files,
        setFiles,
        tags,
        setTags,
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
