interface ProgressBarProps {
  percentage: number;
  stage: string;
  currentChunk: number;
  totalChunks: number;
}

export default function ProgressBar({ 
  percentage, 
  stage, 
  currentChunk, 
  totalChunks 
}: ProgressBarProps) {
  return (
    <div className="mb-3">
      <div className="mb-1 text-sm font-medium text-gray-700">{stage}</div>
      <div className="h-1.5 w-full overflow-hidden rounded bg-gray-200">
        <div 
          className="h-full bg-blue-600 transition-all duration-300 ease-out"
          style={{ width: `${percentage}%` }}
        />
      </div>
      <div className="mt-1 text-xs text-gray-500">
        {currentChunk} / {totalChunks} chunks - {percentage}%
      </div>
    </div>
  );
}