interface ProgressBarProps {
  percentage: number
  stage: string
  currentChunk: number
  totalChunks: number
  details?: string
}

export default function ProgressBar({ percentage, stage, currentChunk, totalChunks, details }: ProgressBarProps) {
  return (
    <div className='mb-3 w-full'>
      <div className='mb-1 flex justify-between text-sm font-medium'>
        <span>{stage}</span>
        <span>{percentage}%</span>
      </div>
      <progress className='progress progress-primary w-full' value={percentage} max='100'></progress>
      <div className='mt-1 flex justify-between text-xs text-base-content/70'>
        <span>
          {currentChunk > 0 ? `${currentChunk} / ${totalChunks} chunks` : ''}
        </span>
        <span>{details}</span>
      </div>
    </div>
  )
}
