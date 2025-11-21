import { InputHTMLAttributes, forwardRef } from 'react';

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  hint?: string;
}

const Input = forwardRef<HTMLInputElement, InputProps>(({ 
  label, 
  hint, 
  className = '', 
  id,
  ...props 
}, ref) => {
  return (
    <div className="flex flex-col gap-2">
      {label && (
        <label htmlFor={id} className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70 text-foreground">
          {label}
        </label>
      )}
      <input
        ref={ref}
        id={id}
        className={`flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 ${className}`}
        {...props}
      />
      {hint && (
        <p className="text-[0.8rem] text-muted-foreground">{hint}</p>
      )}
    </div>
  );
});

Input.displayName = 'Input';

export default Input;