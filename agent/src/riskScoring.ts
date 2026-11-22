export interface RiskDimensionScores {
  smartContractRisk: number; // 1-100
  liquidityRisk: number;     // 1-100
  oracleRisk: number;        // 1-100
  governanceRisk: number;    // 1-100
  centralizationRisk: number;// 1-100
}

export interface RiskWeights {
  smartContract: number; // e.g. 0.30
  liquidity: number;     // e.g. 0.25
  oracle: number;        // e.g. 0.20
  governance: number;    // e.g. 0.15
  centralization: number;// e.g. 0.10
}

export const DEFAULT_RISK_WEIGHTS: RiskWeights = {
  smartContract: 0.30,
  liquidity: 0.25,
  oracle: 0.20,
  governance: 0.15,
  centralization: 0.10,
};

export class ProtocolRiskScoringEngine {
  constructor(private weights: RiskWeights = DEFAULT_RISK_WEIGHTS) {}

  public calculateCompositeRisk(scores: RiskDimensionScores): number {
    const composite =
      scores.smartContractRisk * this.weights.smartContract +
      scores.liquidityRisk * this.weights.liquidity +
      scores.oracleRisk * this.weights.oracle +
      scores.governanceRisk * this.weights.governance +
      scores.centralizationRisk * this.weights.centralization;

    return Math.round(composite * 100) / 100;
  }

  public getRiskCategory(compositeScore: number): 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL' {
    if (compositeScore <= 30) return 'LOW';
    if (compositeScore <= 60) return 'MEDIUM';
    if (compositeScore <= 80) return 'HIGH';
    return 'CRITICAL';
  }

  public shouldTriggerRebalanceAlert(compositeScore: number, threshold = 65): boolean {
    return compositeScore > threshold;
  }
}
