//! Периметр «ваш бизнес» — шаг 0 онбординга (Story 1.2, FR-1).
//!
//! Чистый домен (AD-1): без serde/sqlx/axum. Строковые литералы контракта
//! `/v1` (например, `"marketplace_seller"`) в домене не живут — их маппит
//! шлюз явным match с тестами. Шаг 0 собирает минимум данных и ровно ноль
//! PII: только торговая модель и (для селлеров) список площадок — без имён,
//! телефонов и почт.

/// Торговая модель (вопрос 1 шага 0). Четыре варианта — весь вопросник.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeModel {
    /// Уже продаёт на маркетплейсах.
    MarketplaceSeller,
    /// Планирует выйти на маркетплейсы.
    MarketplacePlanned,
    /// Продаёт только через свои каналы, маркетплейсов не планирует.
    OwnChannelsNoMarketplaces,
    /// Услуги или другой бизнес (не селлер).
    ServicesOther,
}

/// Вердикт периметра: попадает ли бизнес в периметр продукта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Селлер — входит в воронку онбординга (экран «Начните подключение»).
    Seller,
    /// Не-селлер — терминальный исходящий экран с честным отказом.
    NonSeller,
}

/// Ответы шага 0 (минимум данных, ноль PII).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerimeterAnswers {
    /// Торговая модель.
    pub trade_model: TradeModel,
    /// Список площадок — собирается только для селлерских моделей;
    /// для не-селлеров площадки не нужны и не хранятся.
    pub marketplaces: Vec<String>,
}

/// Нарушение инвариантов ответов шага 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerimeterAnswersError {
    /// Селлерская модель торговли требует хотя бы одной площадки.
    EmptyMarketplaces,
}

impl PerimeterAnswers {
    /// Собирает ответы, соблюдая инвариант шага 0.
    ///
    /// Для селлерских моделей (`MarketplaceSeller`, `MarketplacePlanned`)
    /// список площадок непустой ([`PerimeterAnswersError::EmptyMarketplaces`]
    /// иначе). Для не-селлерских моделей площадки не собираются — список
    /// нормализуется в пустой (шаг 0 собирает минимум данных).
    pub fn new(
        trade_model: TradeModel,
        marketplaces: Vec<String>,
    ) -> Result<Self, PerimeterAnswersError> {
        match trade_model {
            TradeModel::MarketplaceSeller | TradeModel::MarketplacePlanned => {
                if marketplaces.is_empty() {
                    return Err(PerimeterAnswersError::EmptyMarketplaces);
                }
                Ok(Self {
                    trade_model,
                    marketplaces,
                })
            }
            TradeModel::OwnChannelsNoMarketplaces | TradeModel::ServicesOther => Ok(Self {
                trade_model,
                marketplaces: Vec::new(),
            }),
        }
    }
}

/// Правило вердикта периметра (FR-1): торговля или план на маркетплейсах —
/// селлер (воронка); свои каналы без маркетплейсов или услуги — не-селлер
/// (честный отказ, в воронку не попадает).
pub fn decide_verdict(answers: &PerimeterAnswers) -> Verdict {
    match answers.trade_model {
        TradeModel::MarketplaceSeller | TradeModel::MarketplacePlanned => Verdict::Seller,
        TradeModel::OwnChannelsNoMarketplaces | TradeModel::ServicesOther => Verdict::NonSeller,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Все 4 правила вердикта (I/O-матрица шага 0).
    #[test]
    fn decide_verdict_all_rules() {
        let cases = [
            (TradeModel::MarketplaceSeller, Verdict::Seller),
            (TradeModel::MarketplacePlanned, Verdict::Seller),
            (TradeModel::OwnChannelsNoMarketplaces, Verdict::NonSeller),
            (TradeModel::ServicesOther, Verdict::NonSeller),
        ];
        for (trade_model, expected) in cases {
            let answers = PerimeterAnswers::new(trade_model, vec!["wildberries".to_owned()])
                .unwrap_or_else(|_| panic!("валидные ответы для {trade_model:?}"));
            assert_eq!(decide_verdict(&answers), expected, "модель {trade_model:?}");
        }
    }

    #[test]
    fn answers_require_marketplace_for_seller_models() {
        for trade_model in [
            TradeModel::MarketplaceSeller,
            TradeModel::MarketplacePlanned,
        ] {
            let err = PerimeterAnswers::new(trade_model, Vec::new())
                .expect_err("селлерская модель без площадок невалидна");
            assert_eq!(err, PerimeterAnswersError::EmptyMarketplaces);
        }
    }

    #[test]
    fn answers_accept_marketplaces_for_seller_models() {
        for trade_model in [
            TradeModel::MarketplaceSeller,
            TradeModel::MarketplacePlanned,
        ] {
            let answers = PerimeterAnswers::new(
                trade_model,
                vec!["wildberries".to_owned(), "ozon".to_owned()],
            )
            .expect("селлерская модель с площадками валидна");
            assert_eq!(answers.marketplaces, ["wildberries", "ozon"]);
            assert_eq!(decide_verdict(&answers), Verdict::Seller);
        }
    }

    #[test]
    fn answers_normalize_empty_marketplaces_for_non_sellers() {
        for trade_model in [
            TradeModel::OwnChannelsNoMarketplaces,
            TradeModel::ServicesOther,
        ] {
            // Площадки для не-селлеров не собираются — даже если пришли.
            let answers = PerimeterAnswers::new(trade_model, vec!["ozon".to_owned()])
                .expect("не-селлерская модель не требует площадок");
            assert!(answers.marketplaces.is_empty());
            assert_eq!(decide_verdict(&answers), Verdict::NonSeller);
        }
    }

    #[test]
    fn non_seller_without_marketplaces_is_valid() {
        let answers = PerimeterAnswers::new(TradeModel::ServicesOther, Vec::new())
            .expect("не-селлер без площадок валиден");
        assert_eq!(decide_verdict(&answers), Verdict::NonSeller);
    }
}
