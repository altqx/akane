'use client'

import React, { createContext, useContext, useState } from 'react'

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
}

const UploadContext = createContext<UploadContextType | undefined>(undefined)

export function UploadProvider({ children }: { children: React.ReactNode }) {
  const [files, setFiles] = useState<File[]>([])
  const [tags, setTags] = useState('')
  const [isUploading, setIsUploading] = useState(false)
  const [progress, setProgress] = useState<ProgressData | null>(null)
  const [results, setResults] = useState<UploadResult[]>([])
  const [error, setError] = useState<string | null>(null)
  const [uploadStatus, setUploadStatus] = useState<string>('')

  // We need a ref to access the latest state inside the async loop if we were using closures,
  // but since we are using state in the loop (reading files[i]), it should be fine as long as files doesn't change during upload.
  // However, to be safe and avoid stale closures if we were to use effects, we'll just use the state directly in the function.

  const startUpload = async () => {
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
      setUploadStatus(`Processing ${i + 1} of ${filesToUpload.length}: ${file.name}`)
      setProgress(null)

      const uploadId = crypto.randomUUID()
      const token = localStorage.getItem('admin_token')

      // Create a promise that resolves when processing is complete
      const processingPromise = new Promise<{ player_url: string; upload_id: string }>((resolve, reject) => {
        const eventSource = new EventSource(`/api/progress/${uploadId}`)
        
        eventSource.onmessage = (event) => {
          try {
            const data: ProgressData = JSON.parse(event.data)
            setProgress(data)

            if (data.status === 'completed' && data.result) {
              eventSource.close()
              resolve(data.result)
            } else if (data.status === 'failed') {
              eventSource.close()
              reject(new Error(data.error || 'Processing failed'))
            }
          } catch (e) {
            console.error('Failed to parse progress data', e)
          }
        }

        eventSource.onerror = () => {
          // If connection drops, we might want to retry or just wait.
          // For now, if it's a hard error, we might reject, but SSE often reconnects.
          // Let's rely on the backend sending a final status.
          // However, if we never get a connection, we should timeout.
        }
      })

      try {
        const formData = new FormData()
        formData.append('file', file)
        formData.append('name', file.name.replace(/\.[^/.]+$/, ''))
        if (tags.trim()) {
          formData.append('tags', tags.trim())
        }

        // Use XMLHttpRequest for upload progress
        const xhr = new XMLHttpRequest()
        
        const uploadPromise = new Promise<void>((resolve, reject) => {
          xhr.upload.addEventListener('progress', (event) => {
            if (event.lengthComputable) {
              const percentComplete = (event.loaded / event.total) * 100
              setProgress(prev => {
                // Only update if we are still in the uploading stage (or haven't started processing)
                if (!prev || prev.stage === 'Uploading to server') {
                  return {
                    stage: 'Uploading to server',
                    percentage: Math.round(percentComplete),
                    current_chunk: 0,
                    total_chunks: 1,
                    details: `Uploaded ${(event.loaded / 1024 / 1024).toFixed(2)} MB`,
                    status: 'uploading'
                  }
                }
                return prev
              })
            }
          })

          xhr.addEventListener('load', () => {
            if (xhr.status >= 200 && xhr.status < 300) {
              resolve()
            } else {
              reject(new Error(xhr.responseText || 'Upload failed'))
            }
          })

          xhr.addEventListener('error', () => reject(new Error('Network error')))
          xhr.addEventListener('abort', () => reject(new Error('Upload aborted')))

          xhr.open('POST', '/api/upload')
          xhr.setRequestHeader('X-Upload-ID', uploadId)
          if (token) {
            xhr.setRequestHeader('Authorization', `Bearer ${token}`)
          }
          xhr.send(formData)
        })

        // Wait for upload to finish
        await uploadPromise
        
        // Wait for processing to finish
        const data = await processingPromise

        const result = {
          file: file.name,
          success: true,
          data: data
        }

        newResults.push(result)
        setResults([...newResults])
      } catch (err: unknown) {
        const errorMessage = err instanceof Error ? err.message : String(err)
        const result = {
          file: file.name,
          success: false,
          error: errorMessage
        }
        newResults.push(result)
        setResults([...newResults])
      }
    }

    setIsUploading(false)
    setUploadStatus('')
    setProgress(null)
    setFiles([])
  }

  const clearUploads = () => {
    setFiles([])
    setResults([])
    setError(null)
    setProgress(null)
    setUploadStatus('')
  }

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
        clearUploads
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
