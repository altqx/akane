'use client'

import { useState, useEffect, useRef } from 'react'
import Button from './Button'
import Input from './Input'

interface Video {
  id: string
  name: string
  tags: string[]
}

interface VideoEditModalProps {
  isOpen: boolean
  onClose: () => void
  video: Video | null
  onSave: (id: string, name: string, tags: string[]) => Promise<void>
}

export default function VideoEditModal({ isOpen, onClose, video, onSave }: VideoEditModalProps) {
  const modalRef = useRef<HTMLDialogElement>(null)
  const [name, setName] = useState('')
  const [tagsInput, setTagsInput] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (video) {
      setName(video.name)
      setTagsInput(video.tags.join(', '))
      setError(null)
    }
  }, [video])

  useEffect(() => {
    const modal = modalRef.current
    if (!modal) return

    if (isOpen) {
      modal.showModal()
    } else {
      modal.close()
    }
  }, [isOpen])

  useEffect(() => {
    const modal = modalRef.current
    if (!modal) return

    const handleClose = () => onClose()
    modal.addEventListener('close', handleClose)
    return () => modal.removeEventListener('close', handleClose)
  }, [onClose])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!video) return

    if (!name.trim()) {
      setError('Name is required')
      return
    }

    setSaving(true)
    setError(null)

    try {
      const tags = tagsInput
        .split(',')
        .map((t) => t.trim())
        .filter((t) => t.length > 0)

      await onSave(video.id, name.trim(), tags)
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save')
    } finally {
      setSaving(false)
    }
  }

  // Handle click outside to close
  const handleBackdropClick = (e: React.MouseEvent<HTMLDialogElement>) => {
    const modal = modalRef.current
    if (!modal) return
    
    const rect = modal.getBoundingClientRect()
    const isInDialog = 
      rect.top <= e.clientY && 
      e.clientY <= rect.top + rect.height &&
      rect.left <= e.clientX && 
      e.clientX <= rect.left + rect.width
    
    if (!isInDialog) {
      onClose()
    }
  }

  if (!isOpen || !video) return null

  return (
    <dialog 
      ref={modalRef} 
      className='modal modal-bottom sm:modal-middle'
      onClick={handleBackdropClick}
    >
      <div className='modal-box'>
        <div className='flex items-center justify-between mb-4'>
          <h3 className='font-bold text-lg'>Edit Video</h3>
          <button 
            className='btn btn-sm btn-circle btn-ghost'
            onClick={onClose}
            disabled={saving}
          >
            ✕
          </button>
        </div>

        {error && (
          <div role='alert' className='alert alert-error mb-4'>
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

        <form onSubmit={handleSubmit}>
          <div className='space-y-4'>
            <Input
              label='Title'
              placeholder='Enter video title'
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={saving}
              required
            />
            <Input
              label='Tags'
              placeholder='Enter tags, separated by commas'
              value={tagsInput}
              onChange={(e) => setTagsInput(e.target.value)}
              disabled={saving}
              hint='e.g., tutorial, gaming, music'
            />
          </div>

          <div className='modal-action'>
            <Button type='button' variant='ghost' onClick={onClose} disabled={saving}>
              Cancel
            </Button>
            <Button type='submit' disabled={saving}>
              {saving ? (
                <>
                  <span className='loading loading-spinner loading-sm'></span>
                  Saving...
                </>
              ) : (
                'Save Changes'
              )}
            </Button>
          </div>
        </form>
      </div>
      <form method='dialog' className='modal-backdrop'>
        <button onClick={onClose}>close</button>
      </form>
    </dialog>
  )
}
