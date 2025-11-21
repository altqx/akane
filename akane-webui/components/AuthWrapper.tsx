'use client'

import { useState } from 'react'
import Input from '@/components/Input'
import Button from '@/components/Button'

export default function AuthWrapper({ children }: { children: React.ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(() => {
    if (typeof window !== 'undefined') {
      const auth = localStorage.getItem('admin_token')
      return !!auth
    }
    return false
  })
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')

    try {
      const res = await fetch('/api/auth/check', {
        headers: {
          Authorization: `Bearer ${password}`
        }
      })

      if (res.ok) {
        localStorage.setItem('admin_token', password)
        setIsAuthenticated(true)
      } else {
        setError('Invalid password')
      }
    } catch (err) {
      console.error(err)
      setError('Login failed')
    }
  }

  if (!isAuthenticated) {
    return (
      <div className='flex min-h-screen items-center justify-center bg-base-200'>
        <div className='card w-full max-w-md bg-base-100 shadow-xl'>
          <div className='card-body'>
            <h2 className='card-title justify-center mb-4 text-2xl font-bold'>Admin Access</h2>
            <form onSubmit={handleLogin} className='flex flex-col gap-4'>
              <Input
                type='password'
                label='Password'
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder='Enter admin password'
              />
              {error && <div className='text-sm text-error'>{error}</div>}
              <div className='card-actions justify-end mt-4'>
                <Button type='submit' className='w-full'>
                  Login
                </Button>
              </div>
            </form>
          </div>
        </div>
      </div>
    )
  }

  return <>{children}</>
}
