// The existing markup and renderers use Russian source strings. Keep their
// originals so switching languages never translates an already translated text.
var ruToEn = {
    'Ожидают проверки': 'Awaiting verification',
    'Доступность токена': 'Token availability',
    "из": "of",
    "включённых": "enabled",
    "выполняются": "running",
    "Отключено:": "Disabled:",
    "Готовы FULL:": "FULL ready:",
    "Каталог:": "Catalog:",
    "Без токена:": "No valid token:",
    "Нужна FULL-проверка:": "Needs FULL check:",
    "Только PREVIEW:": "PREVIEW only:",
    "Пауза 429:": "429 cooldown:",
    "Готовые аккаунты": "Ready accounts",
    "Добавьте аккаунт для доступа к Tidal.": "Add an account to access Tidal.",
    "Готовность: действующий токен и отсутствие паузы 429. Для воспроизведения нужна подтверждённая FULL-проверка не старше 8 часов.": "Readiness requires a valid token and no 429 cooldown. Playback also requires a successful FULL check within the last 8 hours.",
    "Запросы воспроизведения": "Playback requests",
    "В очереди:": "Queued:",
    "Лимит одновременно:": "Concurrent request limit:",
    "Включено аккаунтов:": "Enabled accounts:",
    "Самое долгое ожидание:": "Longest wait:",
    "Нет включённых аккаунтов воспроизведения.": "No playback accounts are enabled.",
    "Запросы треков, видео и лицензий, выполняемые в момент обновления. Лимит задан включёнными аккаунтами; их готовность показана отдельно.": "Track, video, and license requests running at the time of refresh. Enabled accounts determine the limit; their readiness is shown separately.",
    "Источник метаданных": "Metadata source",
    "Аккаунты каталога": "Catalog accounts",
    "Пул воспроизведения": "Playback pool",
    "Токен настроен; доступ проверяется при запросе.": "Token configured; access is checked on request.",
    "Резервных аккаунтов каталога:": "Fallback catalog accounts:",
    "С действующим токеном:": "With a valid token:",
    "Для метаданных FULL-проверка не требуется.": "Metadata does not require a FULL check.",
    "Нет включённых аккаунтов для метаданных.": "No metadata accounts are enabled.",
    "Нет готовых токенов; требуется обновление или завершение паузы.": "No tokens are ready; refresh or cooldown completion is required.",
    "Аккаунты каталога отключены или на паузе 429.": "Catalog accounts are disabled or on 429 cooldown.",
    "Приоритет источников: статичный токен → аккаунты каталога → пул воспроизведения.": "Source priority: static token → catalog accounts → playback pool.",
    "Синхронизация данных": "Data sync",
    "Не настроена": "Not configured",
    "Redis доступен": "Redis reachable",
    "Нет связи с Redis": "Redis unreachable",
    "Общие данные между серверами не синхронизируются.": "Shared data is not synchronized between servers.",
    "Redis отвечает; общие данные синхронизируются.": "Redis is responding; shared data synchronization is enabled.",
    "Сервис использует локальное состояние.": "The service is using local state.",
    "Завершённые API-запросы": "Completed API requests",
    "С момента запуска сервера.": "Since the server started.",
    "Один входящий API-запрос считается один раз, включая внутренние повторы через другие аккаунты.": "Each incoming API request is counted once, including internal retries through other accounts.",
    "Запросов в секунду": "Requests per second",
    "Среднее за последние 60 секунд.": "Average over the last 60 seconds.",
    "Время ответа · p95": "Response time · p95",
    "Запросов в выборке:": "Requests in sample:",
    "95% ответов укладываются в это время.": "95% of responses complete within this time.",
    "Пока нет завершённых запросов.": "No requests have completed yet.",
    "Ответы с ошибками": "HTTP error responses",
    "Доля ответов HTTP 4xx/5xx, включая 404 и 429, среди последних завершённых запросов. Журнал хранит до 5000 записей.": "Share of HTTP 4xx/5xx responses, including 404 and 429, among recent completed requests. The log holds up to 5000 entries.",
    "Готов к каталогу": "Ready for metadata",
    "Готов к FULL": "Ready for FULL",
    "Автоотключён": "Auto-disabled",
    "Отключён вручную": "Manually disabled",
    "Токен отклонён": "Token rejected",
    "Токен истёк": "Token expired",
    "Нет токена": "No token",
    "Пауза 429": "429 cooldown",
    "Только PREVIEW": "PREVIEW only",
    "Нужна FULL-проверка": "Needs FULL check",
    "Статус неизвестен": "Status unknown",
    "Не получен": "Not obtained",
    "Отклонён Tidal": "Rejected by Tidal",
    "FULL · УСТАРЕЛО": "FULL · OUTDATED",
    "FULL подтверждался ранее; нужна новая проверка": "FULL was confirmed earlier; a new check is required",
    "ожидает токен": "waiting for a token",
    "аккаунт отключён": "account disabled",
    "ожидает выполнения": "pending",
    "Обращений с запуска": "Account attempts since start",
    "Ошибок с запуска": "Account errors since start",
    "Обращения к этому аккаунту с запуска сервера, включая повторы для одного API-запроса": "Attempts using this account since server start, including retries for the same API request",
    "Ошибки запросов и обновления токена этого аккаунта с запуска сервера": "Request and token refresh errors for this account since server start",
    "Повтор обновления через": "Retry token refresh in",
    "Восстановление": "Recovery",
    "ожидает попытки": "awaiting retry",
    "Пауза 429 ещё": "429 cooldown remaining",
    "Автоотключён с": "Auto-disabled since",
    "Без работы": "Disabled for",
    "Токен действует ещё": "Token valid for",
    "Последний тест": "Last manual test",
    "не запускался": "not run",
    "Готовы сейчас": "Ready now",
    "Ожидание обновления токена": "Waiting for token refresh",
    "Ошибка авторизации": "Authentication failed",
    "Нет ответа вовремя": "Request timed out",
    "Ошибка проверки": "Test failed",
    "Результат ручной проверки. Подробности по нажатию; текущее состояние — в заголовке аккаунта.": "Manual test result. Select for details; the account header shows current readiness.",
    "Назначения проверены": "Assignments verified",
    "Частично проверены": "Partially verified",
    "Есть доступный прокси": "Proxy available",
    "Проверено назначений": "Verified assignments",
    "Ошибок подряд": "Consecutive errors",
    "Сумма последовательных ошибок по текущим назначениям; сбрасываются после успешного запроса": "Consecutive errors across current assignments; reset after a successful request",
    "Счётчики запросов с запуска сервера. Устаревшие и объединённые ответы входят в попадания. Очистка кэша не обнуляет счётчики.": "Request counters since server start. Stale and coalesced responses are included in hits. Clearing the cache does not reset counters.",
    "Из устаревшего кэша": "Stale cache responses",
    "Ошибок из кэша": "Cached error responses",
    "Объединённых запросов": "Coalesced requests",
    "API-ключи": "API keys",
    "Квота исчерпана": "Quota exhausted",
    "Доступен": "Available",
    "Использовано:": "Used:",
    'Аптайм · последние 7 дней': 'Uptime · last 7 days',
    'Работал': 'Up',
    'Простой': 'Down',
    'Без токена': 'No valid token',
    'Нет действующего токена': 'No valid token available',
    'Зелёный — действующий токен. Жёлтый — токен отсутствует, истёк или отклонён. Красный — аккаунт отключён.': 'Green: valid token. Yellow: token missing, expired, or rejected. Red: account disabled.',
    'Доступность': 'Availability',
    'Процент за период с известным статусом': 'Percentage of the period with a known status',
    'По последнему известному статусу в пуле. Ручное и автоматическое отключение считаются простоем.': 'Based on the last known pool status. Manual and automatic disabling count as downtime.',
    'До начала наблюдения история недоступна.': 'History before monitoring began is unavailable.',
    'Ключ администратора': 'Admin key',
    'Войти': 'Sign in',
    'Панель управления': 'Admin panel',
    'Рабочее пространство': 'Workspace',
    'Основная навигация': 'Main navigation',
    'Обзор': 'Overview',
    'Аккаунты': 'Accounts',
    'Доступ к API': 'API access',
    'Система': 'System',
    'Администратор': 'Administrator',
    '● Сессия активна': '● Session active',
    'Выйти из панели': 'Sign out',
    'Открыть меню': 'Open menu',
    'Состояние сервиса': 'Service status',
    'Управление пулом': 'Pool management',
    'Безопасность': 'Security',
    'Конфигурация': 'Configuration',
    'Tidal-аккаунты, токены и роли каталога.': 'Tidal accounts, tokens, and catalog roles.',
    'Ключи клиентов, квоты и состояние доступа.': 'Client keys, quotas, and access status.',
    'Главные показатели и последние запросы в одном месте.': 'Key metrics and recent requests in one place.',
    'Обновить': 'Refresh',
    '+ Аккаунт': '+ Account',
    'Журнал запросов': 'Request log',
    'Последняя активность обновляется автоматически каждые 15 секунд.': 'Recent activity refreshes automatically every 15 seconds.',
    'Обновить журнал': 'Refresh log',
    'Показать ещё': 'Show more',
    'Доп. поля': 'More fields',
    'Всего': 'Total',
    'Показано': 'Shown',
    'Ошибок': 'Errors',
    '4xx без 429': '4xx excluding 429',
    'Самые медленные:': 'Slowest:',
    'Tidal-аккаунты': 'Tidal accounts',
    'Управляйте пулом воспроизведения, токенами и каталогом.': 'Manage the playback pool, tokens, and catalog.',
    'Проверить все': 'Test all',
    '+ Добавить': '+ Add',
    'Найти аккаунт...': 'Find an account...',
    'Загрузка…': 'Loading…',
    'Результаты проверки': 'Test results',
    'Ключи доступа': 'Access keys',
    'Контролируйте клиентов API и их квоты.': 'Manage API clients and their quotas.',
    'Новый API-ключ': 'New API key',
    'Квота — общее число запросов с этим ключом ко всем маршрутам API (включая поиск и получение трека), без ежедневного сброса. 0 — без лимита. После создания ключ показывается только один раз.': 'The quota is the total number of requests with this key across all API routes, with no daily reset. Use 0 for unlimited access. The key is shown only once after creation.',
    'Название': 'Name',
    'Например, мобильное приложение': 'For example, mobile app',
    'Квота запросов': 'Request quota',
    'Создать ключ': 'Create key',
    'Активные ключи': 'Active keys',
    'Если ключей нет, публичные маршруты API остаются открытыми.': 'Public API routes remain open when there are no keys.',
    'Системные настройки': 'System settings',
    'Интеграции, резервные копии и обслуживание.': 'Integrations, backups, and maintenance.',
    'Настройки панели': 'Panel settings',
    'Язык интерфейса сохраняется в cookie этого браузера.': 'The interface language is saved in a browser cookie.',
    'Язык интерфейса': 'Interface language',
    'Воспроизведение': 'Playback',
    'Настройки выбора формата и автоматического восстановления.': 'Format selection and automatic recovery settings.',
    'Настройки выбора формата и исходящих запросов к Tidal.': 'Format and outgoing Tidal request settings.',
    'Формат по умолчанию': 'Default format',
    'FLAC в приоритете': 'Prefer FLAC',
    'Atmos в приоритете': 'Prefer Atmos',
    'Запросов на аккаунт · треки': 'Requests per account · tracks',
    'Запросов на аккаунт · каталог': 'Requests per account · catalog',
    'Лимит включает первую попытку. При ошибке сервис всё ещё может перейти на другой аккаунт; цепочка аккаунтов в журнале показывает такие переключения.': 'The limit includes the first attempt. On failure, the service may still switch to another account; the account chain in the log shows those failovers.',
    'Автовосстановление отключённых системой аккаунтов': 'Automatically recover system-disabled accounts',
    'Сохранить': 'Save',
    'Очистить очередь': 'Clear queue',
    'Прокси': 'Proxies',
    'Маршрутизация исходящих запросов. Изменения применяются сразу.': 'Route outgoing requests. Changes take effect immediately.',
    'Статус': 'Status',
    'Назначено': 'Assigned',
    'В пуле': 'In pool',
    'Сбоев': 'Failures',
    'Адреса прокси · по одному в строке': 'Proxy addresses · one per line',
    'Включить прокси': 'Enable proxies',
    'Выключить прокси': 'Disable proxies',
    'Сохранить список': 'Save list',
    'Уведомления': 'Notifications',
    'Discord-оповещения о риске блокировки и недоступности аккаунтов.': 'Discord alerts for account bans and outages.',
    'URL вебхука Discord': 'Discord webhook URL',
    'Сохранённый URL скрыт. Введите новый, чтобы заменить его.': 'The saved URL is hidden. Enter a new one to replace it.',
    'Сохранить вебхук': 'Save webhook',
    'Отключить вебхук': 'Disable webhook',
    'Введите URL вебхука.': 'Enter a webhook URL.',
    'Не удалось сохранить вебхук.': 'Could not save webhook.',
    'Вебхук сохранён.': 'Webhook saved.',
    'Удалить сохранённый вебхук и отключить уведомления Discord?': 'Delete the saved webhook and disable Discord alerts?',
    'Не удалось отключить вебхук.': 'Could not disable webhook.',
    'Вебхук отключён.': 'Webhook disabled.',
    'Тест': 'Test',
    'Кэш': 'Cache',
    'Очистка безопасна, но первые ответы после неё могут быть медленнее.': 'Clearing is safe, but the first responses may be slower.',
    'Попадания': 'Hits',
    'Промахи': 'Misses',
    'Устаревшие': 'Stale',
    'Отрицательные': 'Negative',
    'Очистить кэш': 'Clear cache',
    'Учётные данные': 'Credentials',
    'Экспортируйте или импортируйте Tidal-аккаунты в JSON. Дубликаты токенов будут пропущены.': 'Export or import Tidal accounts as JSON. Duplicate tokens are skipped.',
    'Экспорт JSON': 'Export JSON',
    'Импорт JSON': 'Import JSON',
    'База данных': 'Database',
    'Скачайте полный снимок или восстановите состояние без перезапуска.': 'Download a full snapshot or restore without restarting.',
    'Скачать копию': 'Download backup',
    'Восстановить': 'Restore',
    'Новый аккаунт': 'New account',
    'Подключить Tidal': 'Connect Tidal',
    'Закрыть': 'Close',
    'Самый простой вариант — OAuth. Ручной ввод подходит для уже готовых credentials.': 'OAuth is the easiest option. Enter credentials manually if you already have them.',
    'Основной аккаунт': 'Primary account',
    'User ID · необязательно': 'User ID · optional',
    'Только каталог — без воспроизведения': 'Catalog only — no playback',
    'Подключить через OAuth': 'Connect with OAuth',
    'Добавить вручную': 'Add manually',
    'Авторизация через Tidal': 'Authorize with Tidal',
    'Откройте ссылку, войдите в Tidal и подтвердите доступ. Панель сама заметит завершение.': 'Open the link, sign in to Tidal, and approve access. The panel will detect completion.',
    'Копировать': 'Copy',
    'Открыть Tidal': 'Open Tidal',
    'Название аккаунта': 'Account name',
    'Мой Tidal': 'My Tidal',
    'Ожидаем авторизацию…': 'Waiting for authorization…',
    'Отмена': 'Cancel',
    'Редактировать аккаунт': 'Edit account',
    'Результат проверки': 'Test result',
    'Ключ не подходит. Проверьте значение и попробуйте ещё раз.': 'Incorrect key. Check it and try again.',
    'Сессия истекла. Введите ключ ещё раз.': 'Session expired. Enter the key again.',
    'Слишком много неверных попыток. Повторите позже.': 'Too many incorrect attempts. Try again later.',
    'Нет данных': 'No data',
    'в очереди': 'queued',
    'Статичный токен': 'Static token',
    'Каталог': 'Catalog',
    'Общий пул': 'Shared pool',
    'Метаданные через токен': 'Metadata via token',
    'Метаданные через пул воспроизведения': 'Metadata via playback pool',
    'Аккаунты каталога неактивны; используется пул воспроизведения': 'Catalog accounts are inactive; using the playback pool',
    'Без названия': 'Untitled',
    'Синхронизация Redis': 'Redis sync',
    'Один сервер': 'Single server',
    'Работает': 'Connected',
    'Нет связи': 'Disconnected',
    'Проверяем…': 'Testing…',
    'Сначала запустите проверку аккаунтов.': 'Run the account test first.',
    'Всего запросов': 'Total requests',
    'Всего запросов · с запуска': 'Total requests · since start',
    'Запросов/с · 60 с': 'Requests/s · 60s',
    'p95 ответа · до 5000': 'Response p95 · up to 5000',
    'Среднее число завершённых API-запросов в секунду за последние 60 секунд': 'Average completed API requests per second over the last 60 seconds',
    '95% запросов в журнале (до 5000 последних) ответили не медленнее этого значения': '95% of logged requests (up to the last 5000) responded within this time',
    'мс': 'ms',
    'Доля ошибок': 'Error rate',
    'Доля ошибок · до 5000': 'Error rate · up to 5000',
    'Входящие API-запросы с момента запуска сервера; внутренние переключения аккаунтов не дублируют счётчик': 'Incoming API requests since the server started; internal account failovers do not duplicate the count',
    'Доля ответов HTTP 4xx/5xx среди входящих API-запросов в журнале, до 5000 последних': 'Share of HTTP 4xx/5xx responses among incoming API requests in the log, up to the latest 5000',
    'Активные аккаунты': 'Active accounts',
    'Работают сейчас': 'Working now',
    'Включено:': 'Enabled:',
    'Действующий токен без паузы 429; для playback также требуется недавняя успешная FULL-проверка': 'Valid token without a 429 pause; playback also requires a recent successful FULL check',
    'Аккаунтов пока нет': 'No accounts yet',
    'Подключите первый через OAuth — это займёт меньше минуты.': 'Connect the first account with OAuth in under a minute.',
    'Добавить аккаунт': 'Add account',
    'Активен': 'Active',
    'Отключён': 'Disabled',
    'Отключён с': 'Disabled since',
    'Упал': 'Failed at',
    'Спит': 'Inactive for',
    'время неизвестно': 'time unknown',
    'меньше минуты': 'less than a minute',
    'Включён': 'Enabled',
    'Включить': 'Enable',
    'Отключить': 'Disable',
    'Вернуть в пул воспроизведения': 'Return to playback pool',
    'Из каталога': 'Remove from catalog',
    'Использовать только для метаданных': 'Use only for metadata',
    'В каталог': 'Move to catalog',
    'Обновить токен': 'Refresh token',
    'Проверить FULL': 'Check FULL',
    'Проверка FULL/PREVIEW отправляет до четырёх запросов к Tidal': 'FULL/PREVIEW check sends up to four Tidal requests',
    'Доступ к трекам': 'Track access',
    'НЕ ПРИМЕНЯЕТСЯ': 'NOT APPLICABLE',
    'Аккаунт используется только для каталога': 'This account is used for catalog only',
    'Полное воспроизведение подтверждено': 'Full playback confirmed',
    'Доступны только фрагменты': 'Only previews are available',
    'НЕ ОПРЕДЕЛЕНО': 'INCONCLUSIVE',
    'Последняя проверка не дала точного результата': 'The latest check was inconclusive',
    'НЕ ПРОВЕРЕНО': 'NOT CHECKED',
    'Автоматическая проверка ещё не выполнялась': 'The automatic check has not run yet',
    'для каталога не проверяется': 'not checked for catalog accounts',
    'Последняя FULL/PREVIEW проверка': 'Latest FULL/PREVIEW check',
    'Автопроверка FULL/PREVIEW': 'Automatic FULL/PREVIEW check',
    'в течение 5 минут': 'within 5 minutes',
    'Неизвестно': 'Unknown',
    'Не удалось определить': 'Inconclusive',
    'FULL/PREVIEW': 'FULL/PREVIEW',
    'Изменить': 'Edit',
    'Дублировать': 'Duplicate',
    'Удалить': 'Delete',
    'Роль': 'Role',
    'Только каталог': 'Catalog only',
    'Запросов': 'Requests',
    'Автовосстановление': 'Auto recovery',
    'повтор': 'retrying',
    'Токен': 'Token',
    'Проверка': 'Test',
    'Ключей пока нет': 'No keys yet',
    'Публичные маршруты API открыты.': 'Public API routes are open.',
    'Сохраните ключ сейчас — он больше не появится': 'Save this key now — it will not be shown again',
    'API-ключ скопирован.': 'API key copied.',
    'Не удалось скопировать. Выделите ключ и скопируйте вручную.': 'Could not copy. Select and copy the key manually.',
    'Напрямую': 'Direct',
    'Проверяем прокси': 'Checking proxies',
    'Нет рабочего прокси': 'No working proxy',
    'Список и режим сохраняются после перезапуска.': 'The list and mode persist after restart.',
    'Без базы данных настройки действуют до перезапуска.': 'Without a database, settings last until restart.',
    'Не удалось сохранить прокси': 'Could not save proxies',
    'Прокси включены. Проверяем соединение…': 'Proxies enabled. Checking the connection…',
    'Прокси выключены. Используется прямое соединение.': 'Proxies disabled. Using a direct connection.',
    'Список прокси сохранён.': 'Proxy list saved.',
    'Подключён': 'Connected',
    'Не настроен': 'Not configured',
    'активен': 'active',
    'отключён': 'disabled',
    'Аккаунт добавлен.': 'Account added.',
    'Копия': 'Copy of',
    'Сервис временно недоступен: HTTP': 'Service temporarily unavailable: HTTP',
    'Подключаемся к Tidal…': 'Connecting to Tidal…',
    'Откройте ссылку и подтвердите доступ. Ожидаем завершения…': 'Open the link and approve access. Waiting for completion…',
    'Ожидаем подтверждения в браузере…': 'Waiting for approval in the browser…',
    'Скопировано': 'Copied',
    'Укажите Client ID, Client Secret и Refresh Token.': 'Enter Client ID, Client Secret, and Refresh Token.',
    'Удалить этот аккаунт? Он сразу перестанет обслуживать запросы.': 'Delete this account? It will stop handling requests immediately.',
    'Удалить API-ключ? Клиенты с этим ключом потеряют доступ.': 'Delete this API key? Clients using it will lose access.'
};

var enToRu = {};
Object.keys(ruToEn).forEach(function(ru) { enToRu[ruToEn[ru]] = ru; });
Object.assign(enToRu, {
    'Status': 'Статус', 'HTTP Status': 'HTTP-статус', 'Response Time': 'Время ответа',
    'Token Expiry': 'Срок токена', 'Active': 'Активен', 'Yes': 'Да', 'No': 'Нет',
    'Error': 'Ошибка', 'Full JSON Response': 'Полный ответ JSON',
    'PASS': 'УСПЕХ', 'FAIL': 'ОШИБКА',
    'Unknown': 'Неизвестно', 'Account not found': 'Аккаунт не найден',
    'Token refreshed, account reactivated!': 'Токен обновлён, аккаунт снова активен!',
    'Starting...': 'Запуск…', 'Sending...': 'Отправляем…',
    'Report sent!': 'Отчёт отправлен!', 'Test alert sent!': 'Тестовое оповещение отправлено!',
    'Report sent to Discord': 'Отчёт отправлен в Discord',
    'Test alert sent to Discord': 'Тестовое оповещение отправлено в Discord',
    'Response cache cleared': 'Кэш ответов очищен',
    'Backup downloaded': 'Резервная копия скачана',
    'Restore failed': 'Не удалось восстановить базу',
    'Restore database from': 'Восстановить базу из',
    '? Current accounts, keys and settings will be replaced.': '? Текущие аккаунты, ключи и настройки будут заменены.',
    'Clearing...': 'Очищаем…', 'Clear Cache': 'Очистить кэш',
    'Settings saved!': 'Настройки сохранены!',
    'Error saving settings': 'Не удалось сохранить настройки',
    'Import failed': 'Не удалось импортировать данные',
    '$ waiting for traffic…': '$ ожидаем запросы…',
    'hifi-api — live request log': 'hifi-api — журнал запросов',
    'direct': 'напрямую'
});

var languageKey = 'hifi_admin_language';
var adminLanguage = 'en';
var sourceTexts = new WeakMap();
var renderedTexts = new WeakMap();
var sourceAttributes = new WeakMap();
var translatableAttributes = ['placeholder', 'aria-label', 'title'];

function readLanguageCookie() {
    var prefix = languageKey + '=';
    var cookie = document.cookie.split(';').map(function(part) { return part.trim(); })
        .find(function(part) { return part.indexOf(prefix) === 0; });
    var value = cookie ? cookie.slice(prefix.length) : '';
    return value === 'ru' || value === 'en' ? value : null;
}

function localizedCore(source) {
    var dictionary = adminLanguage === 'ru' ? enToRu : ruToEn;
    if (Object.prototype.hasOwnProperty.call(dictionary, source)) return dictionary[source];
    if (adminLanguage === 'en') {
        var match = source.match(/^(\d+) (?:активный аккаунт|активных аккаунта|активных аккаунтов)$/);
        if (match) return match[1] + ' active ' + (Number(match[1]) === 1 ? 'account' : 'accounts');
        match = source.match(/^Циклически \(round-robin\): (.+)$/);
        if (match) return 'Round-robin: ' + match[1];
        match = source.match(/^Резерв: (\d+) (?:аккаунт каталога|аккаунта каталога|аккаунтов каталога)$/);
        if (match) return 'Fallback: ' + match[1] + ' catalog ' + (Number(match[1]) === 1 ? 'account' : 'accounts');
        match = source.match(/^(\d+) (?:аккаунт|аккаунта|аккаунтов)$/);
        if (match) return match[1] + (Number(match[1]) === 1 ? ' account' : ' accounts');
        match = source.match(/^истёк (\d+) мин назад$/);
        if (match) return 'expired ' + match[1] + ' min ago';
        match = source.match(/^истёк (\d+) ч назад$/);
        if (match) return 'expired ' + match[1] + ' hr ago';
        match = source.match(/^(\d+) мин$/);
        if (match) return match[1] + ' min';
        match = source.match(/^(\d+) ч (\d+) мин$/);
        if (match) return match[1] + ' hr ' + match[2] + ' min';
        match = source.match(/^(\d+) д (\d+) ч$/);
        if (match) return match[1] + ' d ' + match[2] + ' hr';
        match = source.match(/^Воспроизведение · (.+)$/);
        if (match) return 'Playback · ' + match[1];
        match = source.match(/^Аккаунт (.+): (FULL|PREVIEW|Не удалось определить)(.*)$/);
        if (match) return 'Account ' + match[1] + ': ' + (ruToEn[match[2]] || match[2]) + match[3];
        match = source.match(/^Очередь очищена: ожидавших (\d+), выполнявшихся (\d+)$/);
        if (match) return 'Queue cleared: ' + match[1] + ' pending, ' + match[2] + ' processing';
        match = source.match(/^Успешно: (\d+) · Ошибок: (\d+) · Нажмите на строку для деталей$/);
        if (match) return 'Passed: ' + match[1] + ' · Failed: ' + match[2] + ' · Select a row for details';
        match = source.match(/^Проверка: (.+)$/);
        if (match) return 'Test: ' + match[1];
        match = source.match(/^Изменить «(.+)»$/);
        if (match) return 'Edit “' + match[1] + '”';
        match = source.match(/^Аккаунт «(.+)» добавлен\.$/);
        if (match) return 'Account “' + match[1] + '” added.';
        match = source.match(/^OK (\d+) мс$/);
        if (match) return 'OK ' + match[1] + ' ms';
        match = source.match(/^Слишком много неверных попыток\. Повторите через (\d+) мин\.$/);
        if (match) return 'Too many incorrect attempts. Try again in ' + match[1] + ' min.';
        match = source.match(/^(.+… · )использовано (.+) · (активен|отключён)$/);
        if (match) return match[1] + 'used ' + match[2] + ' · ' + ruToEn[match[3]];
        var prefixes = {
            'Проверка FULL/PREVIEW: ': 'FULL/PREVIEW check: ',
            'Очистка очереди: ': 'Queue clear: ',
            'Проверка не удалась: ': 'Test failed: ',
            'Ошибка проверки: ': 'Test error: ',
            'Ошибка ': 'Error ',
            'Не удалось завершить сессию: ': 'Could not sign out: ',
            'Не удалось подключиться к сервису: ': 'Could not connect to the service: ',
            'Сервис временно недоступен: ': 'Service temporarily unavailable: ',
            'Копия ': 'Copy of ',
            'Удалить ': 'Delete '
        };
        for (var prefix in prefixes) {
            if (source.startsWith(prefix)) return prefixes[prefix] + source.slice(prefix.length);
        }
    } else {
        var exported = source.match(/^Exported (\d+) accounts$/);
        if (exported) return 'Экспортировано аккаунтов: ' + exported[1];
        var imported = source.match(/^Imported (\d+) accounts \(skipped (\d+)\)$/);
        if (imported) return 'Импортировано: ' + imported[1] + ', пропущено: ' + imported[2];
        var restored = source.match(/^Database restored \((\d+) accounts, (\d+)\/(\d+) healthy\)$/);
        if (restored) return 'База восстановлена: ' + restored[1] + ' аккаунтов, активны ' + restored[2] + '/' + restored[3];
        var englishPrefixes = {
            'Refresh failed: ': 'Не удалось обновить токен: ',
            'Errors: ': 'Ошибки: ',
            'Import error: ': 'Ошибка импорта: ',
            'Export failed: ': 'Не удалось экспортировать данные: ',
            'Backup failed: ': 'Не удалось скачать копию: ',
            'Restore error: ': 'Ошибка восстановления: ',
            'Restore failed: ': 'Не удалось восстановить базу: ',
            'Stats parse: ': 'Ошибка разбора статистики: ',
            'Accounts parse: ': 'Ошибка разбора аккаунтов: ',
            'Stats: ': 'Статистика: ',
            'Accounts: ': 'Аккаунты: ',
            'Failed: ': 'Ошибка: ',
            'Error: ': 'Ошибка: ',
            'Poll error: ': 'Ошибка проверки: '
        };
        for (var englishPrefix in englishPrefixes) {
            if (source.startsWith(englishPrefix)) return englishPrefixes[englishPrefix] + source.slice(englishPrefix.length);
        }
    }
    return source;
}

function tr(source) {
    if (typeof source !== 'string') return source;
    var trimmed = source.trim();
    if (!trimmed) return source;
    var translated = localizedCore(trimmed);
    return translated === trimmed ? source : source.replace(trimmed, translated);
}

function skipTranslation(node) {
    var parent = node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement;
    return parent && parent.closest('script,style,textarea,pre,code,[data-no-i18n],#accounts-container .card-header .label,#keys-container .cred-key,#testResultsList .result-label,#rq-recent .term-path');
}

function localizeText(node, changedByApp) {
    if (skipTranslation(node)) return;
    if (changedByApp && renderedTexts.get(node) === node.nodeValue) return;
    if (changedByApp || !sourceTexts.has(node)) sourceTexts.set(node, node.nodeValue);
    var output = tr(sourceTexts.get(node));
    if (node.nodeValue !== output) {
        renderedTexts.set(node, output);
        node.nodeValue = output;
    }
}

function localizeAttributes(element) {
    if (skipTranslation(element)) return;
    var originals = sourceAttributes.get(element);
    if (!originals) { originals = {}; sourceAttributes.set(element, originals); }
    translatableAttributes.forEach(function(attribute) {
        if (!element.hasAttribute(attribute)) return;
        if (!(attribute in originals)) originals[attribute] = element.getAttribute(attribute);
        var output = tr(originals[attribute]);
        if (element.getAttribute(attribute) !== output) element.setAttribute(attribute, output);
    });
}

function localizeTree(root) {
    if (root.nodeType === Node.TEXT_NODE) { localizeText(root, false); return; }
    if (root.nodeType !== Node.ELEMENT_NODE || skipTranslation(root)) return;
    localizeAttributes(root);
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
        var node = walker.currentNode;
        if (node.nodeType === Node.TEXT_NODE) localizeText(node, false);
        else localizeAttributes(node);
    }
}

function setLanguage(language) {
    adminLanguage = language === 'ru' ? 'ru' : 'en';
    document.cookie = languageKey + '=' + adminLanguage + '; Path=/admin; Max-Age=31536000; SameSite=Lax' +
        (location.protocol === 'https:' ? '; Secure' : '');
    document.documentElement.lang = adminLanguage;
    document.title = adminLanguage === 'ru' ? 'HiFi API — управление' : 'HiFi API — Admin';
    var select = document.getElementById('panel-language');
    if (select) select.value = adminLanguage;
    localizeTree(document.body);
    if (typeof refreshUptimeCharts === 'function') refreshUptimeCharts();
    if (window._stats && typeof overviewCards === 'function') document.getElementById('stats').innerHTML = overviewCards(window._stats);
}

function initLanguage() {
    var saved = readLanguageCookie();
    if (!saved) {
        try { saved = localStorage.getItem(languageKey); } catch (_) {}
    }
    setLanguage(saved);
    try { localStorage.removeItem(languageKey); } catch (_) {}
    new MutationObserver(function(mutations) {
        mutations.forEach(function(mutation) {
            if (mutation.type === 'characterData') {
                localizeText(mutation.target, true);
            } else {
                mutation.addedNodes.forEach(localizeTree);
            }
        });
    }).observe(document.body, { childList: true, characterData: true, subtree: true });
}
