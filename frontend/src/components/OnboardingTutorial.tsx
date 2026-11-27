'use client';

import React, { useState, useEffect, useRef } from 'react';
import { X, ChevronRight, ChevronLeft, Wallet, TrendingUp, ArrowDownCircle, PieChart, CheckCircle } from 'lucide-react';

interface TutorialStep {
  id: string;
  title: string;
  description: string;
  icon: React.ReactNode;
  target?: string;
}

const STEPS: TutorialStep[] = [
  {
    id: 'welcome',
    title: 'Welcome to NeuroWealth',
    description: 'AI-powered DeFi yield optimization on Stellar. Let us show you around.',
    icon: <TrendingUp className="w-6 h-6" />,
  },
  {
    id: 'connect-wallet',
    title: 'Connect Your Wallet',
    description: 'Connect your Freighter or Albedo wallet to start earning yield on your USDC.',
    icon: <Wallet className="w-6 h-6" />,
    target: 'connect-wallet-btn',
  },
  {
    id: 'strategies',
    title: 'AI-Powered Strategies',
    description: 'Our AI agent automatically rebalances between DeFi protocols to maximize your yield.',
    icon: <PieChart className="w-6 h-6" />,
  },
  {
    id: 'deposit',
    title: 'Make Your First Deposit',
    description: 'Deposit USDC into the vault. The AI will deploy it across yield-generating protocols.',
    icon: <ArrowDownCircle className="w-6 h-6" />,
    target: 'deposit-btn',
  },
  {
    id: 'portfolio',
    title: 'Track Your Portfolio',
    description: 'Watch your earnings grow in real-time. View detailed analytics and transaction history.',
    icon: <TrendingUp className="w-6 h-6" />,
  },
];

const COMPLETION_KEY = 'neurowealth-onboarding-completed';

function focusableIn(root: HTMLElement): HTMLElement[] {
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((node) => !node.hasAttribute('disabled'));
}

export function OnboardingTutorial() {
  const [isOpen, setIsOpen] = useState(false);
  const [currentStep, setCurrentStep] = useState(0);
  const [isCompleted, setIsCompleted] = useState(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const completed = localStorage.getItem(COMPLETION_KEY);
    if (!completed) {
      const timer = setTimeout(() => setIsOpen(true), 1500);
      return () => clearTimeout(timer);
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    previouslyFocused.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const targetId = STEPS[currentStep]?.target;
    const target = targetId ? document.getElementById(targetId) : null;
    if (target) {
      target.scrollIntoView({ block: 'nearest' });
      target.style.outline = '2px solid rgb(16 185 129)';
      target.style.outlineOffset = '4px';
    }
    const nodes = dialog ? focusableIn(dialog) : [];
    (nodes[0] ?? dialog)?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        localStorage.setItem(COMPLETION_KEY, 'true');
        setIsOpen(false);
        return;
      }
      if (event.key !== 'Tab' || !dialog) return;
      const items = focusableIn(dialog);
      if (items.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      if (target) {
        target.style.outline = '';
        target.style.outlineOffset = '';
      }
      previouslyFocused.current?.focus();
    };
  }, [isOpen, currentStep]);

  const handleNext = () => {
    if (currentStep < STEPS.length - 1) {
      setCurrentStep(currentStep + 1);
    } else {
      complete();
    }
  };

  const handlePrev = () => {
    if (currentStep > 0) {
      setCurrentStep(currentStep - 1);
    }
  };

  const complete = () => {
    localStorage.setItem(COMPLETION_KEY, 'true');
    setIsCompleted(true);
    setTimeout(() => setIsOpen(false), 500);
  };

  const skip = () => {
    localStorage.setItem(COMPLETION_KEY, 'true');
    setIsOpen(false);
  };

  if (!isOpen || isCompleted) return null;

  const step = STEPS[currentStep];
  const progress = ((currentStep + 1) / STEPS.length) * 100;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-title"
        aria-describedby="onboarding-description"
        tabIndex={-1}
        className="glass-panel rounded-2xl p-6 max-w-md w-full mx-4 relative outline-none"
      >
        <button
          onClick={skip}
          className="absolute top-4 right-4 text-slate-400 hover:text-white transition-colors"
          aria-label="Skip tutorial"
        >
          <X size={20} />
        </button>

        <div className="mb-6">
          <div className="flex items-center justify-between text-xs text-slate-400 mb-2">
            <span>Step {currentStep + 1} of {STEPS.length}</span>
            <button onClick={skip} className="hover:text-emerald-400 transition-colors">
              Skip tour
            </button>
          </div>
          <div className="h-1 bg-slate-800 rounded-full overflow-hidden">
            <div
              className="h-full bg-emerald-500 transition-all duration-300"
              style={{ width: `${progress}%` }}
            />
          </div>
        </div>

        <div className="flex flex-col items-center text-center mb-6">
          <div className="p-4 rounded-2xl bg-emerald-500/10 text-emerald-400 mb-4">
            {step.icon}
          </div>
          <h3 id="onboarding-title" className="text-xl font-bold text-white mb-2">{step.title}</h3>
          <p id="onboarding-description" className="text-slate-400 text-sm leading-relaxed">{step.description}</p>
        </div>

        <div className="flex items-center justify-between">
          <button
            onClick={handlePrev}
            disabled={currentStep === 0}
            className="flex items-center gap-1 px-4 py-2 text-sm text-slate-400 hover:text-white disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
          >
            <ChevronLeft size={16} />
            Back
          </button>

          <button
            onClick={handleNext}
            className="flex items-center gap-1 px-6 py-2.5 bg-emerald-500 hover:bg-emerald-600 text-white font-medium rounded-xl transition-colors"
          >
            {currentStep === STEPS.length - 1 ? (
              <>
                Get Started
                <CheckCircle size={16} />
              </>
            ) : (
              <>
                Next
                <ChevronRight size={16} />
              </>
            )}
          </button>
        </div>

        <div className="flex justify-center gap-1.5 mt-6">
          {STEPS.map((_, i) => (
            <button
              key={i}
              onClick={() => setCurrentStep(i)}
              className={`w-2 h-2 rounded-full transition-all duration-200 ${
                i === currentStep
                  ? 'bg-emerald-400 w-6'
                  : i < currentStep
                  ? 'bg-emerald-500/50'
                  : 'bg-slate-600'
              }`}
              aria-label={`Go to step ${i + 1}`}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
