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

/**
 * Identity established by trusted auth middleware (#1119). Auth middleware must
 * set `res.locals.authenticatedUserId` after verifying a JWT/session. Client
 * headers such as `x-user-id` are never trusted: they are attacker-controlled
 * and rotating them would bypass the limit.
 */
export function getVerifiedUserId(res: Response): string | undefined {
  const id = res.locals?.authenticatedUserId;
  return typeof id === 'string' && id.trim() ? id.trim() : undefined;
}

// Per-User rate limiter. Keys only off a verified identity; unauthenticated
// requests fall back to the client IP (the IP limiter still applies).
export const userRateLimiter = rateLimit({
  windowMs,
  max: userMax,
  standardHeaders: true,
  legacyHeaders: true,
  keyGenerator: (req: Request, res: Response) => {
    const userId = getVerifiedUserId(res);
    if (userId) return `user:${userId}`;
    return `ip:${req.ip || 'unknown'}`;
  },
  handler: (_req: Request, res: Response) => {
    res.setHeader('Retry-After', Math.ceil(windowMs / 1000).toString());
    res.status(429).json({
      error: 'Too many requests for this user account, please try again later.',
      retryAfterSeconds: Math.ceil(windowMs / 1000),
    });
  },
});
