'use client'

import { useEffect, useRef } from 'react'

interface VideoPreviewModalProps {
  isOpen: boolean
  onClose: () => void
  playerUrl: string
  videoName: string
}

export default function VideoPreviewModal({ isOpen, onClose, playerUrl, videoName }: VideoPreviewModalProps) {
  const modalRef = useRef<HTMLDialogElement>(null)

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

  if (!isOpen) return null

  return (
    <dialog 
      ref={modalRef} 
      className='modal modal-bottom sm:modal-middle'
      onClick={handleBackdropClick}
    >
      <div className='modal-box w-11/12 max-w-4xl p-0 overflow-hidden'>
        <div className='flex items-center justify-between p-4 border-b border-base-300'>
          <h3 className='font-bold text-lg truncate max-w-[80%]' title={videoName}>
            {videoName}
          </h3>
          <button 
            className='btn btn-sm btn-circle btn-ghost'
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className='aspect-video w-full bg-black'>
          <iframe
            src={playerUrl}
            className='w-full h-full'
            allowFullScreen
            title={videoName}
          />
        </div>
      </div>
      <form method='dialog' className='modal-backdrop'>
        <button onClick={onClose}>close</button>
      </form>
    </dialog>
  )
}
