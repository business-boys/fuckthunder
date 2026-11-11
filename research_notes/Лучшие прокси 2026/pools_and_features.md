# Пулы и возможности residential/mobile proxy-провайдеров — ноябрь 2026

## 1. У кого самые большие заявленные residential и mobile пулы?

### Takeaway
По актуальному официальному заявлению лидирует **Bright Data: 400M+ residential IP в месяц**, но это месячная уникальная ёмкость, а не число одновременно доступных адресов — [Bright Data, проверено 10.11.2026](https://docs.brightdata.com/products/residential/introduction). Среди провайдеров с актуальными раздельными mobile-заявлениями лидирует **Oxylabs — 20M+ IP**; цифры SOAX противоречивы, а старые данные NetNut нельзя считать текущими — [Oxylabs, проверено 10.11.2026](https://oxylabs.io/products/mobile-proxies); [SOAX residential, обновлено 20.10.2026](https://soax.com/proxies/residential); [SOAX mobile, обновлено 20.10.2026](https://soax.com/proxies/mobile); [Google GTIG, 03.07.2026](https://cloud.google.com/blog/topics/threat-intelligence/google-continued-disruption-residential-proxy-networks).

### Cited Findings
Все маркетинговые числа ниже — **заявления самих поставщиков**, если отдельно не указано иное. Дата «проверено» обозначает дату обращения к странице, а не независимый аудит пула.

| Провайдер | Residential pool и покрытие | Mobile pool и покрытие | Статус и источник |
|---|---|---|---|
| **Bright Data** | **400M+ IP в месяц**, **195+ стран** | **7M+ IP** | Актуальные официальные данные, проверено 10.11.2026: [residential documentation](https://docs.brightdata.com/products/residential/introduction), [pricing/proxy network](https://brightdata.com/pricing/proxy-network). Старые официальные comparison pages со **150M residential** устарели относительно текущей документации и не должны использоваться как текущее значение — [пример: Bright Data/DataImpulse comparison, проверено 10.11.2026](https://brightdata.com/solutions/dataimpulse-alternative). |
| **Oxylabs** | **175M+ IP**, **195 стран** | **20M+ IP**, **140+ стран** | Актуальные официальные данные, проверено 10.11.2026: [residential](https://oxylabs.io/products/residential-proxy-pool), [mobile](https://oxylabs.io/products/mobile-proxies). Oxylabs определяет размер residential-пула как число уникальных exit nodes за день, умноженное на число дней квартала; это не одновременный live pool — [methodology на странице residential](https://oxylabs.io/products/residential-proxy-pool). |
| **Decodo** (бывший Smartproxy) | **115M+ IP**, **195+ локаций** | **10M+ IP**, **160+ локаций**, **700+ carriers** | Актуальные официальные данные, проверено 10.11.2026: [residential](https://decodo.com/proxies/residential-proxies), [mobile](https://decodo.com/proxies/mobile). Официальный rebranding Smartproxy → Decodo состоялся **22.04.2025** — [Decodo](https://decodo.com/smartproxy). |
| **SOAX** | Отдельный актуальный residential pool не раскрыт; residential-страница заявляет **12M IP в совокупном residential+mobile pool**, **195+ стран** | Mobile-страница одновременно заявляет **191M IP в совокупном residential+mobile pool**, **195+ стран** | Прямое противоречие двух официальных страниц, обе обновлены **20.10.2026** и проверены 10.11.2026: [residential — 12M](https://soax.com/proxies/residential), [mobile — 191M](https://soax.com/proxies/mobile). Нельзя ни сложить эти цифры, ни выбрать одну как достоверную без уточнения поставщика. |
| **NetNut** | **85M** — лишь зафиксированное Proxyway заявление на март–апрель 2026 | **5M** — лишь зафиксированное Proxyway заявление на март–апрель 2026 | Оба значения **устарели** после июльского disruption и не являются текущим доступным пулом — [Proxyway, данные собраны в марте–апреле 2026](https://proxyway.com/research/proxy-market-research-2026). **02.07.2026** Alarum подтвердила seizure части доменов NetNut со стороны FBI — [Alarum/SEC, 02.07.2026](https://www.sec.gov/Archives/edgar/data/1725332/000121390026075170/ea029706002ex99-1.htm); **03.07.2026** Google GTIG сообщила о координированном disruption сети не менее чем из **2M устройств** и сокращении доступных устройств на миллионы — [Google GTIG](https://cloud.google.com/blog/topics/threat-intelligence/google-continued-disruption-residential-proxy-networks). |
| **IPRoyal** | **64M+ IP**, **195+ стран** | **4.5M+ IP**; точное общее число стран на mobile-странице не указано | Актуальные официальные данные, проверено 10.11.2026: [residential](https://iproyal.com/residential-proxies/), [mobile](https://iproyal.com/mobile-proxies/). |
| **DataImpulse** | **90M+ IP**, **195 стран** | **16M+ IP**, **195 стран** | Актуальные официальные данные, проверено 10.11.2026: [residential](https://dataimpulse.com/residential-proxies/), [mobile](https://dataimpulse.com/mobile-proxies/). Добавлен как сильный дополнительный конкурент: его заявления превышают соответствующие пулы Decodo, IPRoyal и старые показатели NetNut. |

### Inferences
- **По residential headline claim:** Bright Data — условный №1 (400M+ monthly IP), Oxylabs — №2 (175M+), Decodo — №3 (115M+), DataImpulse — №4 (90M+), IPRoyal — следующий (64M+). SOAX и NetNut в этот актуальный рейтинг корректно не вставить: у первого конфликтуют официальные данные, у второго нет надёжного post-disruption значения.
- **По mobile headline claim:** Oxylabs — лидер среди провайдеров с непротиворечивым актуальным заявлением (20M+), затем DataImpulse (16M+), Decodo (10M+), Bright Data (7M+), IPRoyal (4.5M+). SOAX нельзя ранжировать по 191M, пока поставщик не объяснит противоречие с 12M.
- Разные знаменатели (`monthly unique`, квартальная экстраполяция, совокупный residential+mobile pool) делают простое сравнение чисел неточным. Это рейтинг **рекламируемой ёмкости**, а не доказанный рейтинг одновременно живых, чистых или доступных в конкретной стране IP.

### Gaps
- Ни один поставщик не публикует сопоставимый, независимо проверенный снимок **одновременно доступных** residential и mobile IP с одинаковой методологией на 10.11.2026.
- Bright Data не раскрывает на использованных страницах разбивку 7M+ mobile IP по странам; IPRoyal не даёт точного общего mobile country count.
- SOAX не предоставил согласованного разделения своего совокупного пула на residential и mobile и не объяснил разницу 12M против 191M.
- Текущий размер пула NetNut после событий июля 2026 не установлен; публичный сайт не дал проверяемых актуальных количественных данных.

## 2. Как различаются покрытие, rotation/sticky sessions, протоколы и типы продуктов?

### Takeaway
**Oxylabs и Bright Data** дают самый широкий заявленный набор residential-фильтров, **Decodo** сочетает широкое покрытие с HTTP/3/MASQUE, а **IPRoyal** выделяется официально заявленными residential sticky sessions до семи дней — [Oxylabs](https://oxylabs.io/products/residential-proxy-pool); [Bright Data](https://docs.brightdata.com/products/residential/configure-your-proxy); [Decodo](https://decodo.com/proxies/residential-proxies); [IPRoyal](https://iproyal.com/residential-proxies/) (все страницы проверены 10.11.2026). Заявленный sticky limit — верхняя настройка, а не гарантия жизни конкретного peer/IP.

### Cited Findings
| Провайдер | Residential: targeting, rotation и sticky | Mobile: sessions и targeting | Протоколы и релевантные типы продуктов |
|---|---|---|---|
| **Bright Data** | Country/state/city, US ZIP и ASN targeting; rotating и sticky sessions — [configuration docs, проверено 10.11.2026](https://docs.brightdata.com/products/residential/configure-your-proxy). | Отдельный **Mobile Proxies** pool; заявлено 7M+ IP — [pricing/proxy network, проверено 10.11.2026](https://brightdata.com/pricing/proxy-network). | Residential documentation заявляет **HTTP(S), SOCKS5**; отдельные продукты **Residential Proxies** и **Mobile Proxies** — [introduction](https://docs.brightdata.com/products/residential/introduction), [configuration](https://docs.brightdata.com/products/residential/configure-your-proxy). |
| **Oxylabs** | Country/state/city/ZIP/coordinates/ASN, опциональные OS и IP-version filters; rotation на каждый request, sticky **до 24 ч** — [residential, проверено 10.11.2026](https://oxylabs.io/products/residential-proxy-pool). | Rotation и sticky **до 24 ч**; покрытие 140+ стран — [mobile, проверено 10.11.2026](https://oxylabs.io/products/mobile-proxies). | Residential: **HTTP(S), HTTP/3, MASQUE, SOCKS5**; раздельные residential/mobile products — [residential](https://oxylabs.io/products/residential-proxy-pool), [mobile](https://oxylabs.io/products/mobile-proxies). |
| **Decodo** | Rotation на каждый request и sticky sessions; 195+ локаций — [residential, проверено 10.11.2026](https://decodo.com/proxies/residential-proxies). Proxyway в марте–апреле 2026 зафиксировал sticky **1 мин–24 ч**, default 10 мин и auto-rotation после 60 с inactivity — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). | **700+ carriers**, 160+ локаций; rotating mobile product — [mobile, проверено 10.11.2026](https://decodo.com/proxies/mobile). | **HTTP(S), SOCKS5**, а также **HTTP/3 QUIC/MASQUE**; раздельные residential/mobile products — [residential](https://decodo.com/proxies/residential-proxies), [mobile](https://decodo.com/proxies/mobile). |
| **SOAX** | Rotation per request / после N requests / по timer / при ошибке; sticky mode с фиксацией node; 195+ стран — [residential, обновлено 20.10.2026](https://soax.com/proxies/residential). | Общий с residential пул и mobile targeting; из-за конфликта размер не используем как надёжный параметр — [mobile, обновлено 20.10.2026](https://soax.com/proxies/mobile). | **HTTP(S), SOCKS5**; раздельные продуктовые страницы residential/mobile, но **объединённый** заявленный pool — [residential](https://soax.com/proxies/residential), [mobile](https://soax.com/proxies/mobile). |
| **IPRoyal** | Rotation и sticky **до 7 дней**; 195+ стран — [residential, проверено 10.11.2026](https://iproyal.com/residential-proxies/). | **Rotating** и **dedicated mobile** варианты, custom rotation, carrier/geo targeting — [mobile, проверено 10.11.2026](https://iproyal.com/mobile-proxies/). | Residential поддерживает **HTTP(S), SOCKS5**; отдельные residential, rotating mobile и dedicated mobile options — [residential](https://iproyal.com/residential-proxies/), [mobile](https://iproyal.com/mobile-proxies/). |
| **DataImpulse** | Rotation и sticky sessions; 195 стран — [residential, проверено 10.11.2026](https://dataimpulse.com/residential-proxies/). Proxyway в марте–апреле 2026 зафиксировал residential sticky **1–120 мин**, default 30 мин — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). | Rotation и sticky **до 120 мин**; 195 стран — [mobile, проверено 10.11.2026](https://dataimpulse.com/mobile-proxies/). | **HTTP(S), SOCKS5**; раздельные residential/mobile products — [residential](https://dataimpulse.com/residential-proxies/), [mobile](https://dataimpulse.com/mobile-proxies/). |
| **NetNut** | В мартовско-апрельском тесте были rotation per request и sessions «as long as available», long sessions до 60 мин; **HTTP, HTTPS, SOCKS5** — [Proxyway, март–апрель 2026](https://proxyway.com/research/proxy-market-research-2026). | Отдельный mobile pool участвовал в тесте Proxyway, но его старое заявление 5M не отражает текущее состояние — [Proxyway](https://proxyway.com/research/proxy-market-research-2026); [Google GTIG, 03.07.2026](https://cloud.google.com/blog/topics/threat-intelligence/google-continued-disruption-residential-proxy-networks). | Исторически residential/mobile/shared datacenter networks; после July 2026 disruption эти данные нельзя считать подтверждением текущей доступности — [Proxyway](https://proxyway.com/research/proxy-market-research-2026); [Alarum/SEC, 02.07.2026](https://www.sec.gov/Archives/edgar/data/1725332/000121390026075170/ea029706002ex99-1.htm). |

Дополнительные ограничения сравнения:

- В исследовании Proxyway residential sticky limits составляли у Decodo и Oxylabs до 24 ч, у DataImpulse до 120 мин, у SOAX от 10 с до 60 мин; тестер отдельно отмечает, что поведение зависит от того, остаётся ли устройство online — [Proxyway, март–апрель 2026](https://proxyway.com/research/proxy-market-research-2026).
- UDP/QUIC в 2026 встречается реже, чем базовый SOCKS5, и у части поставщиков требует специального включения; поэтому наличие SOCKS5 само по себе не означает поддержку UDP — [Proxyway, март–апрель 2026](https://proxyway.com/research/proxy-market-research-2026).

### Inferences
- Для сложной геолокализации residential-трафика первыми стоит тестировать **Bright Data и Oxylabs**; **Decodo** — сильная альтернатива, особенно если нужны современные HTTP/3/MASQUE options.
- Для длинных residential sessions **IPRoyal** интересен заявленными 7 днями, но фактическую устойчивость нужно проверять на целевых ASN и гео: peer может уйти offline раньше лимита.
- Для mobile-задач **Oxylabs** сочетает крупный актуальный pool, 140+ стран и sticky до 24 ч; **Decodo** полезен широким carrier targeting; **IPRoyal** стоит выделять не размером, а наличием dedicated mobile варианта.
- Протокол и максимальный session TTL не определяют успешность сами по себе: качество конкретного exit IP, его репутация, гео, ASN и защита целевого сайта остаются отдельными переменными.

### Gaps
- Не все официальные страницы раскрывают одинаково подробные mobile TTL, список протоколов и гранулярность targeting; пустая ячейка в таком сравнении не доказывает отсутствие функции.
- Нет единого независимого теста, подтверждающего заявленные максимальные sticky durations на одинаковых гео и нагрузке после октября–ноября 2026.
- Актуальная функциональность NetNut после July 2026 disruption не подтверждена; строка выше намеренно опирается на датированный исторический тест.

## 3. Что показывают независимые тесты и кого разумно рекомендовать?

### Takeaway
Независимые замеры **не подтверждают автоматически победу самого большого рекламного числа**: в тестах Proxyway марта–апреля 2026 Oxylabs лидировал по уникальным global residential/mobile IP, а Decodo — по US uniqueness — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). Практическая рекомендация: **Bright Data** — по крупнейшему актуальному residential headline claim, **Oxylabs** — как наиболее подкреплённый тестами выбор для обоих типов, **Decodo** — сильная альтернатива; NetNut в текущий shortlist по размеру не включать.

### Cited Findings
**Что именно измерял Proxyway**

- Основная часть данных собрана в **марте–апреле 2026**; residential global test: **3.6M requests за 21 день**, US: **1.2M requests за 14 дней**; mobile global: **1.2M requests**, US: **280K requests**. Поставщики заранее получили доступ и методологию, поэтому результаты лучше трактовать как их настроенную/лучшую конфигурацию, а не обязательно default self-service experience — [Proxyway methodology](https://proxyway.com/research/proxy-market-research-2026).
- **Bright Data не участвовал** в сравнении 2026, поэтому его 400M+ monthly claim нельзя напрямую сопоставить с measured unique IP других участников — [Proxyway](https://proxyway.com/research/proxy-market-research-2026); [Bright Data documentation](https://docs.brightdata.com/products/residential/introduction).

**Residential: advertised против наблюдаемого**

| Метрика Proxyway | Результат | Значение для сравнения |
|---|---|---|
| Наибольший advertised pool среди участников | **Oxylabs — 175M** | Это был рейтинг заявлений участников на момент исследования, не всех существующих провайдеров — [Proxyway, март–апрель 2026](https://proxyway.com/research/proxy-market-research-2026). |
| Global unique IP | **Oxylabs — 3,241,404 unique IP / 3.6M requests** | Лучший наблюдаемый global result среди участников, но не census всего пула — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |
| US unique IP | **Decodo — 961,218 unique IP / 1.2M requests** | Большой global headline не гарантирует лидерство в каждом отдельном гео — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |
| Global infrastructure success | **Oxylabs — 99.93%**, **Decodo — 99.92%**, **SOAX — 99.78%**, **DataImpulse — 99.74%** | Это success к простому CDN endpoint, не success против защищённой цели — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |
| Тогдашние advertised SOAX и NetNut | **SOAX — 155M**, **NetNut — 85M** | Исторические значения марта–апреля 2026: SOAX теперь имеет конфликтующие официальные 12M/191M combined claims, NetNut пережил July disruption; не использовать как актуальные — [Proxyway](https://proxyway.com/research/proxy-market-research-2026); [SOAX residential](https://soax.com/proxies/residential); [SOAX mobile](https://soax.com/proxies/mobile); [Google GTIG](https://cloud.google.com/blog/topics/threat-intelligence/google-continued-disruption-residential-proxy-networks). |

**Mobile: advertised против наблюдаемого**

| Метрика Proxyway | Результат | Значение для сравнения |
|---|---|---|
| Тогдашний наибольший advertised mobile pool | **SOAX — 33M** | Только историческое заявление, зафиксированное в марте–апреле 2026; текущие официальные страницы SOAX дают конфликтующие совокупные 12M/191M — [Proxyway](https://proxyway.com/research/proxy-market-research-2026); [SOAX](https://soax.com/proxies/mobile). |
| Global unique IP | **Oxylabs — 1,192,449 unique IP / 1.2M requests** | Сильнейшее независимое свидетельство наблюдаемой global mobile ёмкости среди участников — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |
| US unique IP | **Decodo — 152,424 unique IP / 280K requests** | Поддерживает включение Decodo в shortlist для US mobile наряду с Oxylabs — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |
| Global infrastructure success | **Oxylabs — 99.84%** | Результат инфраструктурного теста, а не обещание прохождения конкретного anti-bot — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |
| DataImpulse target benchmark | **85.31% average success** на Amazon/Google/Instagram в их тесте | Полезный сигнал для пилота, но узкий набор целей и дат не позволяет переносить процент на любой сайт — [Proxyway](https://proxyway.com/research/proxy-market-research-2026). |

**Операционный риск**

- Google GTIG **03.07.2026** сообщила о продолжении disruption residential proxy networks: по её оценке, затронутая NetNut-ассоциированная сеть включала не менее **2M устройств**, а число доступных устройств сократилось на миллионы — [Google Threat Intelligence Group](https://cloud.google.com/blog/topics/threat-intelligence/google-continued-disruption-residential-proxy-networks).
- Alarum **02.07.2026** подтвердила seizure определённых доменов NetNut и сотрудничество с law enforcement; это первичное корпоративное подтверждение инцидента, но не публикация нового размера пула — [Alarum exhibit к SEC filing](https://www.sec.gov/Archives/edgar/data/1725332/000121390026075170/ea029706002ex99-1.htm).

### Inferences
- **Если критерий строго «самый большой актуальный официально заявленный residential pool»:** выбрать **Bright Data (400M+ monthly IP)**, обязательно сохранив оговорку о месячной методике подсчёта и отсутствии участия в Proxyway 2026.
- **Если критерий «лучшее сочетание размера и независимо наблюдаемой ёмкости»:** **Oxylabs** — наиболее защищённая рекомендация и для residential, и для mobile. **Decodo** особенно убедителен для US uniqueness и является разумным вторым поставщиком для A/B-теста.
- **DataImpulse** заслуживает места в расширенном shortlist благодаря 90M/16M заявлениям и сильным отдельным benchmark results, но не обгоняет Bright Data/Oxylabs по соответствующим headline pools.
- **IPRoyal** рационально рассматривать для длинных sticky sessions или dedicated mobile, а не как лидера по числу IP. **SOAX** имеет гибкие rotation controls и хорошие исторические infrastructure results, но конфликт pool claims требует письменного уточнения перед выводами о размере.
- **NetNut** исключён из актуальных рекомендаций «largest pool»: старые 85M/5M нельзя переносить через July 2026 disruption без новых проверяемых данных.
- Финальный выбор должен делаться по платному/ограниченному пилоту на нужных странах, carriers/ASN и целевых сайтах: unique IP rate, повторяемость, latency, session survival и target-specific success. Ни pool size, ни benchmark **не гарантируют отсутствие блокировок или 100% success**.

### Gaps
- Proxyway — независимый, но ограниченный по времени, гео, числу запросов и участникам benchmark; он не измеряет полный pool и не включает Bright Data в 2026.
- Нет независимых post-July 2026 измерений NetNut и нет независимого объяснения текущего противоречия SOAX 12M/191M.
- Нет сопоставимых публичных данных, которые позволили бы сертифицировать крупнейший **одновременно доступный**, **чистый** или **не заблокированный конкретным сайтом** pool.
- Для окончательного выбора не хватает одинакового ноябрьского A/B-теста всех shortlisted providers на конкретных целевых ресурсах пользователя; без него возможны только обоснованный shortlist и сравнение заявлений, не гарантия результата.
