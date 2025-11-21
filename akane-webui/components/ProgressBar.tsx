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
      <div className="mb-1 text-sm font-medium text-foreground">{stage}</div>
      <div className="h-2 w-full overflow-hidden rounded-full bg-secondary">
        <div 
          className="h-full bg-primary transition-all duration-300 ease-out"
          style={{ width: `${percentage}%` }}
        />
      </div>
      <div className="mt-1 text-xs text-muted-foreground">
        {currentChunk} / {totalChunks} chunks - {percentage}%
      </div>
    </div>
  );
}