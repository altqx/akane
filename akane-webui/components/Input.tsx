import { InputHTMLAttributes, forwardRef } from 'react'

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string
  hint?: string
}

const Input = forwardRef<HTMLInputElement, InputProps>(({ label, hint, className = '', id, ...props }, ref) => {
  return (
    <label className='form-control w-full'>
      {label && (
        <div className='label'>
          <span className='label-text'>{label}</span>
        </div>
      )}
      <input
        ref={ref}
        id={id}
        className={`input input-bordered w-full ${className}`}
        {...props}
      />
      {hint && (
        <div className='label'>
          <span className='label-text-alt text-muted-foreground'>{hint}</span>
        </div>
      )}
    </label>
  )
})

Input.displayName = 'Input'

export default Input
