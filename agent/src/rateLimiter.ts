import { rateLimit } from 'express-rate-limit';
import { Request, Response } from 'express';

const windowMs = parseInt(process.env.RATE_LIMIT_WINDOW_MS || '60000', 10);
const ipMax = parseInt(process.env.RATE_LIMIT_MAX_REQUESTS || '30', 10);
const userMax = parseInt(process.env.USER_RATE_LIMIT_MAX_REQUESTS || '20', 10);

// Per-IP rate limiter
export const ipRateLimiter = rateLimit({
  windowMs,
  max: ipMax,
  standardHeaders: true,
  legacyHeaders: true,
  handler: (_req: Request, res: Response) => {
    res.setHeader('Retry-After', Math.ceil(windowMs / 1000).toString());
    res.status(429).json({
      error: 'Too many requests from this IP, please try again later.',
      retryAfterSeconds: Math.ceil(windowMs / 1000),
    });
  },
});

// Per-User rate limiter (reads X-User-ID or user-id header, falls back to IP)
export const userRateLimiter = rateLimit({
  windowMs,
  max: userMax,
  standardHeaders: true,
  legacyHeaders: true,
  keyGenerator: (req: Request) => {
    const userId = req.headers['x-user-id'] || req.headers['user-id'];
    if (typeof userId === 'string' && userId.trim()) {
      return `user:${userId.trim()}`;
    }
    return req.ip || 'unknown';
  },
  handler: (_req: Request, res: Response) => {
    res.setHeader('Retry-After', Math.ceil(windowMs / 1000).toString());
    res.status(429).json({
      error: 'Too many requests for this user account, please try again later.',
      retryAfterSeconds: Math.ceil(windowMs / 1000),
    });
  },
});
