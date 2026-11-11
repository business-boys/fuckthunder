# Лучшие residential- и mobile-прокси 2026: цены и сценарии выбора

Проверено 10 ноября 2026 года. Все суммы — USD, если не указано иное. Цены провайдеров — публичные тарифы без индивидуальных скидок; VAT/налоги, платный геотаргетинг и дополнительные функции могут увеличить счёт. Размеры пулов — **заявления провайдеров**, а не независимо измеренное число одновременно доступных уникальных IP. «Цена/GB» не равна стоимости успешного запроса: важны оба направления трафика, retry, тяжёлые ответы и правила списания.

## 1. Сколько реально стоит начать и масштабироваться?

### Takeaway

- **Самый дешёвый осмысленный тест:** DataImpulse — $5 за 5 GB residential или 2,5 GB mobile, трафик не сгорает.
- **Простой небольшой subscription:** Decodo residential — от $11,25/мес. за 3 GB; PAYG — $4/GB + VAT. Mobile сейчас показывает promo-цены, которые нельзя считать постоянными.
- **Предсказуемый enterprise-onboarding:** Oxylabs — от $15/мес.; цена падает с ростом пакета, но неиспользованные кредиты обычно сгорают в биллинговом цикле.
- **Трафик без срока годности:** IPRoyal и DataImpulse; у IPRoyal вход дороже ($7,35 за 1 GB residential PAYG).
- **SOAX** требует бюджет от $200/мес. + VAT и выгоднее при смешанных гео, а не для разового теста.
- **Bright Data** нельзя уверенно сверить по первоисточнику: сайт вернул 403, а актуальная рекламная выдача противоречит майскому обзору Proxyway.
- **NetNut исключён из текущего шортлиста:** домен показывает сообщение об изъятии FBI, а не тарифы.

### Cited Findings

| Провайдер | Residential: вход и основные тарифы | Mobile: вход и основные тарифы | Срок трафика / trial |
|---|---|---|---|
| **Oxylabs** | Shared monthly credits: Starter **$15/мес., $6/GB**; Basic **$100, $5/GB**; Flexible **$500, $4/GB**; Enterprise **от $2 500, $2,50/GB**. | Те же пакеты: **$7,50 / $6,67 / $5 / $3,50 за GB** соответственно; Starter фактически даёт 2 GB. | Welcome credit **$5**. Неиспользованные кредиты истекают каждый биллинговый цикл, кроме переноса при upgrade. Тарифицируются request + response независимо от HTTP status. [Цены](https://oxylabs.io/pricing), [mobile](https://oxylabs.io/products/mobile-proxies). |
| **Decodo / Smartproxy** | PAYG **$4/GB + VAT**; подписки: **3 GB $11,25/мес.**, 10 GB $35, 25 GB $81,25, 50 GB $150, 100 GB $275, 500 GB $1 250, 1 TB $2 500. | На странице действует **50% sale**: PAYG **$4/GB + VAT** (FAQ всё ещё указывает обычные $8); **2 GB $7,50/мес.**, 8 GB $28, 25 GB $81,30, 50 GB $150, 100 GB $275. | Trial **3 дня или 100 MB**, затем автоматически становится платным; 14-day refund имеет исключения по использованию/trial. PAYG покупает по 1 GB из Wallet, автоматически докупает следующий при 90% расхода; GB живут 12 месяцев и PAYG несовместим с monthly subscription. [Residential](https://decodo.com/pricing), [mobile](https://decodo.com/proxies/mobile/pricing), [биллинг](https://decodo.com/faq/billing/how-does-the-monthly-subscription-model-work). |
| **SOAX** | Builder **$200/мес. + VAT**: Tier 1 **$3/GB**, Tier 2 **$2,25**, Tier 3 **$1,20**. Team $500: $2,20/$1,65/$0,90. Scale $1 500: $1,50/$1,15/$0,60. Enterprise $3 000: от $0,85/$0,55/$0,35. | **Те же тарифы**, что residential. | Sandbox бесплатен, но включённых кредитов нет; это не обычный free trial. Monthly credits действуют 60 дней, annual — 12 месяцев. Оба направления трафика тарифицируются. US = Tier 1, India = Tier 2. [Цены и billing FAQ](https://soax.com/pricing). |
| **IPRoyal** | PAYG: **1 GB $7,35**, 2 GB по $6,25/GB, 10 GB по $5,51/GB, 1 TB $2,43/GB, 10 TB $1,84/GB. Subscription: 1 GB $7, 2 GB $5,95/GB, 10 GB $5,25/GB, 1 TB $2,30/GB, 10 TB $1,75/GB. | Rotating: 2 GB **$6,80/GB** ($13,60), 10 GB $6/GB, 50 GB $5,60/GB, 100 GB $5,20/GB. Dedicated: **$10,11/день** или $130/30 дней; $123,50/мес. при 60 днях, $117/мес. при 90 днях. | Residential traffic **never expires**, нет monthly minimum и обязательного контракта. [Residential](https://iproyal.com/pricing/residential-proxies/), [mobile](https://iproyal.com/pricing/mobile-proxies/). |
| **DataImpulse** | Intro **$5/5 GB = $1/GB**; **$50/50 GB = $1/GB**; **$800/1 TB = $0,80/GB**; custom от $4 000 для 5 TB+. Premium residential — $5/GB. | Intro **$5/2,5 GB = $2/GB**; **$50/25 GB = $2/GB**; **$1 600/1 TB = $1,60/GB**. | Нет подписки; трафик **не истекает**. Advanced geo-targeting может оплачиваться отдельно. [Официальная страница](https://dataimpulse.com/). |
| **Bright Data** | Официальная страница недоступна для проверки (403). Proxyway, обновлено 8 мая 2026: PAYG **$4/GB**, commitments $500/$1 000/$2 000; поисковая реклама официальной страницы одновременно обещает «Starting from $5,88/GB». | Текущая официальная цена не подтверждена. | По Proxyway: **7-дневный trial для компаний**. [Независимый контекст](https://proxyway.com/reviews/bright-data-proxies), [официальная страница, требующая перепроверки](https://brightdata.com/pricing/proxy-network/residential-proxies). |
| **NetNut** | Недоступны: при прямом визите `netnut.io/pricing/` отображается **“Seized by Federal Bureau Investigation”**. | Недоступны. | Не считать действующим предложением. Proxyway сообщает о disruption со стороны FBI/Google в июле 2026 и что сервис сейчас не работает. [Независимый контекст](https://proxyway.com/reviews/netnut-proxies), [недоступный сайт](https://netnut.io/pricing/). |

**Как читать дешёвые «от $/GB»:** $0,80/GB у DataImpulse требует пакета $800; $1,84/GB у IPRoyal — уровня 10 TB; $2,50/GB у Oxylabs — Enterprise от $2 500; минимальные $/GB у SOAX зависят и от дорогого плана, и от географического tier. Это не цены маленького стартового пакета.

### Inferences

1. Для разового дешёвого benchmark DataImpulse минимизирует денежный риск; IPRoyal удобнее, если тесты растянуты и важнее сохранение остатка, чем минимальный чек.
2. Decodo выглядит сбалансированным для небольшого регулярного расхода, но trial 100 MB годится лишь на smoke test. Перед покупкой mobile стоит зафиксировать в заказе, действует ли скидка и какой будет renewal price.
3. При 100 GB/мес. номинальные пакеты дают ориентир: Decodo residential/mobile $275, Oxylabs Flexible $500 с остатком кредитов либо индивидуальный расчёт, SOAX зависит от смеси tier, IPRoyal требует checkout-расчёта. Нельзя ранжировать их только по минимальной рекламной цене.
4. Для mixed-geo SOAX надо считать взвешенную цену: например, на Builder 70 GB US и 30 GB India обойдутся в `70×$3 + 30×$2,25 = $277,50` сверх доступности в рамках плана/кредитов; без проверки правил включённого объёма это лишь стоимость трафика, а не итоговый invoice.
5. При тяжёлых запросах двунаправленный биллинг Oxylabs/SOAX особенно важен. Для экономики проекта метрика — `все списания / число валидных результатов`, включая повторы, а не номинальный $/GB.

### Gaps

- Bright Data: актуальные official residential/mobile tiers, expiry, условия trial и refund требуют ручной проверки в браузере или письменного quote; $4 и $5,88/GB нельзя молча объединять.
- Не получены полные официальные правила refund для каждого провайдера и налоговый итог для конкретной юрисдикции.
- У Decodo promo mobile может закончиться; у DataImpulse цена advanced geo-targeting зависит от опций. У крупных enterprise-планов фактическая цена определяется контрактом.
- У NetNut нет оснований приводить исторические цены как действующие.

## 2. Что известно о пуле, KYC и эксплуатационных ограничениях?

### Takeaway

Большой advertised pool не гарантирует нужную страну, ASN, sticky session или низкий повтор IP. Oxylabs, SOAX и IPRoyal публикуют понятные KYC-требования; Decodo и DataImpulse требуют отдельной проверки триггеров до оплаты. Для любого провайдера нужно заранее согласовать целевые сайты и use case.

### Cited Findings

| Провайдер | Заявленный пул / покрытие | KYC и ограничения |
|---|---|---|
| **Oxylabs** | **175M+ residential IP**; **20M+ mobile IP**, 140+ стран для mobile. | Все клиенты отвечают на KYC questionnaire; запрашиваются сведения о компании и use case. [KYC & Safety](https://oxylabs.io/kyc-and-safety), [mobile pool](https://oxylabs.io/products/mobile-proxies). |
| **Decodo** | **115M+ residential IP**; **10M+ mobile IP**, 700+ carriers, 160+ locations. | Точный перечень KYC-триггеров в проверенных страницах не зафиксирован. Trial автоматически конвертируется в платный план; refund не универсален. [Residential](https://decodo.com/pricing), [mobile](https://decodo.com/proxies/mobile/pricing). |
| **SOAX** | В проверенной pricing-странице не использовано как сопоставимая числовая метрика. | Verification обязателен **до paid access**; возможны owner/entity, tax, beneficial ownership и ID documents, а также ongoing re-verification. [KYC/KYB Policy](https://soax.com/legal/kyc-kyb-policy), [pricing](https://soax.com/pricing). |
| **IPRoyal** | **64M+ residential IP**; **4,5M+ mobile IP**. | Проверки начинаются при регистрации; identity verification открывает unrestricted features. [KYC Policy](https://iproyal.com/kyc-policy/), [residential](https://iproyal.com/pricing/residential-proxies/), [mobile](https://iproyal.com/pricing/mobile-proxies/). |
| **DataImpulse** | **90M+ residential IP** и **16M+ mobile IP** по данным главной страницы. | Точные KYC-триггеры не подтверждены; advanced geo-targeting может стоить extra. [Официальная страница](https://dataimpulse.com/). |
| **Bright Data** | Proxyway указывает advertised **400M residential IP**; это не наш независимый замер. | Proxyway характеризует KYC как strict и указывает company trial. Текущую official policy следует перепроверить. [Proxyway](https://proxyway.com/reviews/bright-data-proxies). |
| **NetNut** | Не оценивать как доступный пул. | Сайт seized/unavailable; операционный и compliance-статус не подходит для нового production-контракта. [Proxyway](https://proxyway.com/reviews/netnut-proxies). |

### Inferences

- **Enterprise / формальный procurement:** Oxylabs и Bright Data логично ставить в первый список, но для Bright Data сначала закрыть пробел official quote. Oxylabs уже даёт прозрачную стартовую точку и публичную KYC-модель.
- **Когда заранее известен бюджет на compliance:** SOAX требует времени на verification до оплаты и бюджета $200+, зато geo-tier pricing может быть экономичен при дешёвых странах.
- **Когда нужен небольшой или нерегулярный объём:** Decodo, IPRoyal и DataImpulse проще сравнивать фактическим тестом. Меньший advertised pool IPRoyal не означает худший результат на узкой гео — решает выборка именно нужных ASN/городов.
- **Mobile:** Decodo заявляет наиболее явно описанное carrier-покрытие среди доступных страниц; DataImpulse дешевле на входе, IPRoyal предлагает ещё и dedicated вариант. Но нельзя утверждать, что один из них реже попадает в блокировки без одинакового target-теста.
- Не использовать pool size как proxy для легальности или качества sourcing. В procurement стоит отдельно запросить consent/sourcing policy, запрещённые категории трафика и порядок abuse-расследований.

### Gaps

- Нет единой методики независимого аудита pool size, churn, уникальности IP, доли реальных mobile/residential устройств и качества конкретной гео.
- Не подтверждены детальные KYC-триггеры Decodo и DataImpulse, а также актуальная официальная KYC-страница Bright Data.
- В таблицах нет гарантий success rate, uptime, concurrency, sticky-session TTL или доступности конкретного города/ASN: эти параметры надо фиксировать в тесте/договоре.
- Не делается вывод о «банах» по названию бренда: вероятность блокировки зависит от target, поведения, fingerprint, rate limits и конкретного пула.

## 3. Кого выбрать под сценарий и как проверить до контракта?

### Takeaway

Практичный шортлист: **Oxylabs / Bright Data** — enterprise-кандидаты; **Decodo** — умеренный бюджет и большой заявленный пул; **DataImpulse** — самый дешёвый пробный объём и non-expiring traffic; **IPRoyal** — нерегулярное потребление без сгорания residential GB; **SOAX** — production со смешанной географией и бюджетом от $200/мес. **NetNut сейчас не рассматривать.**

### Cited Findings

| Сценарий | Первичный кандидат | Альтернатива и причина |
|---|---|---|
| Минимальный paid benchmark | **DataImpulse:** $5, трафик не сгорает ([source](https://dataimpulse.com/)). | **Oxylabs Starter:** $15 + $5 welcome credit, но важны expiry и двунаправленный биллинг ([source](https://oxylabs.io/pricing)). |
| Нерегулярные residential-задачи | **IPRoyal:** non-expiring traffic, без monthly minimum ([source](https://iproyal.com/pricing/residential-proxies/)). | **DataImpulse:** дешевле на входе и тоже без expiry ([source](https://dataimpulse.com/)). |
| Небольшой регулярный пакет | **Decodo:** 3 GB $11,25 или 10 GB $35 residential ([source](https://decodo.com/pricing)). | **Oxylabs:** $15 Starter, особенно если важен enterprise-путь роста ([source](https://oxylabs.io/pricing)). |
| Enterprise procurement | **Oxylabs:** публичные tiers и KYC ([pricing](https://oxylabs.io/pricing), [KYC](https://oxylabs.io/kyc-and-safety)). | **Bright Data:** большой advertised pool по Proxyway, но только после official quote ([context](https://proxyway.com/reviews/bright-data-proxies)). |
| Смешанные страны, крупный production | **SOAX:** одинаковые residential/mobile tier rates; дешёвые Tier 2/3 ([source](https://soax.com/pricing)). | **Oxylabs/DataImpulse:** сравнить по фактическому набору стран и успешному результату. |
| Mobile с отдельным выделенным ресурсом | **IPRoyal dedicated mobile:** $130/30 дней ([source](https://iproyal.com/pricing/mobile-proxies/)). | Rotating mobile Decodo/DataImpulse/Oxylabs — если dedicated не обязателен. |

### Inferences

Рекомендуемый порядок выбора:

1. **Отбросить недоступное и неподтверждённое:** NetNut — не включать; Bright Data — запросить официальный quote, не полагаться на сниппет.
2. **Провести одинаковый paid test у 2–3 кандидатов**, не сравнивая trial одного с production-пулом другого. Базовая тройка по бюджету: DataImpulse, Decodo/IPRoyal, Oxylabs; SOAX добавить при mixed-geo и достаточном объёме.
3. **Считать effective cost:** `(оплата трафика + обязательная подписка + geo/ASN add-ons + налоги) / валидные результаты`. Отдельно сохранить GB запросов, GB ответов и retry traffic.
4. **Проверить качество по сегментам:** success по HTTP и бизнес-валидации ответа; retries/request; p50/p95/p99 latency; TTFB; гео и ASN accuracy; sticky-session TTL и смену IP; уникальность/повторяемость IP; concurrency; ошибки по каждому target и стране.
5. **Проверить договорно:** оба направления billing, rounding, минимальный чек, expiry, auto-renew/auto-top-up, renewal после promo, refund, лимиты и paid targeting. Отдельно получить письменное подтверждение допустимости use case/targets.
6. **Проверить операционку:** KYC/KYB сроки, поддержку и escalation, sourcing/consent documentation, abuse policy, возможность выбрать/исключить ASN и стабильность пула в повторном тесте через 1–2 недели.

Итоговое решение должно опираться на стоимость **валидного ответа в нужной гео**, а не на pool size или минимальный рекламный $/GB. Для нерегулярного проекта expiry может перевесить разницу в тарифе; для постоянного production — retry rate, поддержка и договорные ограничения важнее $5 стартового чека.

### Gaps

- Без единого живого теста нельзя назвать абсолютного победителя по success rate, скорости, чистоте пула или частоте блокировок.
- Нужны официальные подтверждения Bright Data и KYC-деталей Decodo/DataImpulse; enterprise-цены требуют quote.
- Не рассчитан итоговый TCO под конкретные страны, GB, число запросов, средний размер request/response и допустимый retry rate — для этого нужны параметры проекта.
- Перед оплатой необходимо перепроверить promo, доступность сервиса и условия: особенно Decodo mobile и статус NetNut.
