// The existing markup and renderers use Russian source strings. Keep their
// originals so switching languages never translates an already translated text.
var ruToEn = {
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
    'Всего': 'Total',
    'Ошибок': 'Errors',
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
    'Язык интерфейса сохраняется в этом браузере.': 'The interface language is saved in this browser.',
    'Язык интерфейса': 'Interface language',
    'Воспроизведение': 'Playback',
    'Настройки выбора формата и автоматического восстановления.': 'Format selection and automatic recovery settings.',
    'Формат по умолчанию': 'Default format',
    'FLAC в приоритете': 'Prefer FLAC',
    'Atmos в приоритете': 'Prefer Atmos',
    'Автовосстановление отключённых системой аккаунтов': 'Automatically recover system-disabled accounts',
    'Сохранить': 'Save',
    'Прокси': 'Proxies',
    'Маршрутизация исходящих запросов. Изменения применяются сразу.': 'Route outgoing requests. Changes take effect immediately.',
    'Статус': 'Status',
    'Текущий': 'Current',
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
    'Синхронизация Redis': 'Redis sync',
    'Один сервер': 'Single server',
    'Работает': 'Connected',
    'Нет связи': 'Disconnected',
    'Проверяем…': 'Testing…',
    'Сначала запустите проверку аккаунтов.': 'Run the account test first.',
    'Всего запросов': 'Total requests',
    'Доля ошибок': 'Error rate',
    'Активные аккаунты': 'Active accounts',
    'Аккаунтов пока нет': 'No accounts yet',
    'Подключите первый через OAuth — это займёт меньше минуты.': 'Connect the first account with OAuth in under a minute.',
    'Добавить аккаунт': 'Add account',
    'Активен': 'Active',
    'Отключён': 'Disabled',
    'Включён': 'Enabled',
    'Включить': 'Enable',
    'Отключить': 'Disable',
    'Вернуть в пул воспроизведения': 'Return to playback pool',
    'Из каталога': 'Remove from catalog',
    'Использовать только для метаданных': 'Use only for metadata',
    'В каталог': 'Move to catalog',
    'Обновить токен': 'Refresh token',
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

function localizedCore(source) {
    var dictionary = adminLanguage === 'ru' ? enToRu : ruToEn;
    if (Object.prototype.hasOwnProperty.call(dictionary, source)) return dictionary[source];
    if (adminLanguage === 'en') {
        var match = source.match(/^(\d+) (?:аккаунт|аккаунта|аккаунтов)$/);
        if (match) return match[1] + (Number(match[1]) === 1 ? ' account' : ' accounts');
        match = source.match(/^истёк (\d+) мин назад$/);
        if (match) return 'expired ' + match[1] + ' min ago';
        match = source.match(/^истёк (\d+) ч назад$/);
        if (match) return 'expired ' + match[1] + ' hr ago';
        match = source.match(/^(\d+) мин$/);
        if (match) return match[1] + ' min';
        match = source.match(/^(\d+) ч (\d+) мин$/);
        if (match) return match[1] + ' hr ' + match[2] + ' min';
        match = source.match(/^Воспроизведение · (.+)$/);
        if (match) return 'Playback · ' + match[1];
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
    try { localStorage.setItem(languageKey, adminLanguage); } catch (_) {}
    document.documentElement.lang = adminLanguage;
    document.title = adminLanguage === 'ru' ? 'HiFi API — управление' : 'HiFi API — Admin';
    var select = document.getElementById('panel-language');
    if (select) select.value = adminLanguage;
    localizeTree(document.body);
}

function initLanguage() {
    try { adminLanguage = localStorage.getItem(languageKey) === 'ru' ? 'ru' : 'en'; }
    catch (_) { adminLanguage = 'en'; }
    setLanguage(adminLanguage);
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
