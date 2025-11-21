'use client';

import { useState } from 'react';
import Input from '@/components/Input';
import Button from '@/components/Button';

export default function AuthWrapper({
  children,
}: {
  children: React.ReactNode;
}) {
  const [isAuthenticated, setIsAuthenticated] = useState(() => {
    if (typeof window !== 'undefined') {
      const auth = localStorage.getItem('admin_token');
      return !!auth;
    }
    return false;
  });
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');

    try {
      const res = await fetch('/api/auth/check', {
        headers: {
          'Authorization': `Bearer ${password}`
        }
      });

      if (res.ok) {
        localStorage.setItem('admin_token', password);
        setIsAuthenticated(true);
      } else {
        setError('Invalid password');
      }
    } catch (err) {
      console.error(err);
      setError('Login failed');
    }
  };

  if (!isAuthenticated) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-background">
        <div className="w-full max-w-md rounded-lg border border-border bg-card p-8 shadow-sm text-card-foreground">
          <h1 className="mb-6 text-center text-2xl font-bold">Admin Access</h1>
          <form onSubmit={handleLogin} className="flex flex-col gap-4">
            <Input
              type="password"
              label="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter admin password"
            />
            {error && <div className="text-sm text-destructive">{error}</div>}
            <Button type="submit">Login</Button>
          </form>
        </div>
      </div>
    );
  }

  return <>{children}</>;
}