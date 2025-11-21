'use client'

import React, { createContext, useContext, useState } from 'react'

export interface UploadResult {
  file: string
  success: boolean
  data?: {
    playlist_url: string
    upload_id: string
  }
  error?: string
}

export interface ProgressData {
  percentage: number
  stage: string
  current_chunk: number
  total_chunks: number
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

  const pollProgress = async (uploadId: string) => {
    try {
      const token = localStorage.getItem('admin_token')
      const res = await fetch(`/api/progress/${uploadId}`, {
        headers: {
          Authorization: `Bearer ${token}`
        }
      })
      if (!res.ok) return
      const data = await res.json()
      if (data) {
        setProgress(data)
      }
    } catch (err) {
      console.error('Progress poll error:', err)
    }
  }

  const startUpload = async () => {
    if (files.length === 0) {
      setError('Please select at least one video file.')
      return
    }

    setIsUploading(true)
    setResults([])
    setError(null)

    const newResults: UploadResult[] = []

    // We iterate over a copy of the files array to ensure stability
    const filesToUpload = [...files]

    for (let i = 0; i < filesToUpload.length; i++) {
      const file = filesToUpload[i]
      setUploadStatus(`Uploading ${i + 1} of ${filesToUpload.length}: ${file.name}`)
      setProgress(null)

      // Generate a client-side ID for progress tracking
      const uploadId = crypto.randomUUID()

      // Start polling immediately
      const pollInterval = setInterval(() => {
        pollProgress(uploadId)
      }, 500)

      try {
        const formData = new FormData()
        formData.append('file', file)
        formData.append('name', file.name.replace(/\.[^/.]+$/, ''))
        if (tags.trim()) {
          formData.append('tags', tags.trim())
        }

        // Start upload request with X-Upload-ID header
        const token = localStorage.getItem('admin_token')
        const res = await fetch('/api/upload', {
          method: 'POST',
          headers: {
            'X-Upload-ID': uploadId,
            Authorization: `Bearer ${token}`
          },
          body: formData
        })

        if (!res.ok) {
          const text = await res.text()
          throw new Error(text || 'Upload failed')
        }

        const data = await res.json()

        const result = {
          file: file.name,
          success: true,
          data: data
        }

        newResults.push(result)
        // Update results state progressively so user sees success as they happen
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
      } finally {
        clearInterval(pollInterval)
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
