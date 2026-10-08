import { getCurrentWindow } from "@tauri-apps/api/window";
import { backend } from "./bridge";
import {
  BUZZ_HEADER_COLUMN_GAP,
  BUZZ_HEADER_MIN_PROFILE_WIDTH,
  buzzBalanceTooltip,
  buzzKindLabel,
  completedSyncChanged,
  fitBuzzFontSize,
  formatCompactBuzz,
  formatExactBuzz,
  visibleBuzzBalances,
} from "./buzz";
import { button, element, formatDate, formatNumber, iconButton, isAllowedImageUrl, relativeTime } from "./dom";
import { playSound } from "./audio";
import appIcon from "./assets/icon128.png";
import { loadCivitaiImage } from "./images";
import {
  isModelUpdateNotification,
  isSubmissionUpdateNotification,
  notificationMatchesFilter,
} from "./notification-filters";
import type {
  AppSnapshot,
  BuzzAccountType,
  BuzzTransactionItem,
  Filter,
  NotificationItem,
  NotificationSounds,
  PermissionStatus,
  SettingsPage,
  SoundId,
  View,
} from "./types";

const FILTERS: ReadonlyArray<{ id: Filter; label: string; iconClass?: string }> = [
  { id: "all", label: "ALL" },
  { id: "comments", label: "Comments and replies", iconClass: "icon-comment" },
  { id: "followers", label: "New followers", iconClass: "icon-user" },
  { id: "milestones", label: "Reaction milestones", iconClass: "icon-reaction" },
  { id: "tips", label: "Buzz tips", iconClass: "icon-bolt" },
  { id: "models", label: "Models Updates", iconClass: "icon-model-grid" },
  { id: "updates", label: "Miscellaneous", iconClass: "icon-bell" },
];

const SETTINGS_PAGES: ReadonlyArray<{ id: Exclude<SettingsPage, "root">; title: string; detail: string }> = [
  { id: "account", title: "Account & Security", detail: "API key, connection and permissions" },
  { id: "notifications", title: "Notifications & Sounds", detail: "Windows alerts and sound groups" },
  { id: "behavior", title: "Behavior", detail: "Polling, window behavior and Windows startup" },
  { id: "privacy", title: "Privacy & Data", detail: "Preferences backup and local data" },
  { id: "about", title: "About", detail: "Version, privacy and build details" },
];

export class CompanionApp {
  private snapshot: AppSnapshot;
  private view: View = "notifications";
  private settingsPage: SettingsPage = "root";
  private filter: Filter = "all";
  private busy = false;
  private scrollPositions = new Map<string, {
    scrollTop: number;
    anchor: string | null;
    anchorOffset: number;
  }>();
  private buzzAccountType: BuzzAccountType = "blue";
  private buzzPages: Record<BuzzAccountType, {
    items: BuzzTransactionItem[];
    nextCursor: string | null;
    loaded: boolean;
    loading: boolean;
    error: string;
    revision: number;
  }> = {
    blue: { items: [], nextCursor: null, loaded: false, loading: false, error: "", revision: 0 },
    yellow: { items: [], nextCursor: null, loaded: false, loading: false, error: "", revision: 0 },
    green: { items: [], nextCursor: null, loaded: false, loading: false, error: "", revision: 0 },
  };
  constructor(private readonly root: HTMLElement, snapshot: AppSnapshot) {
    this.snapshot = snapshot;
  }

  async start(): Promise<void> {
    await backend.onStateChanged(({ snapshot, soundId }) => {
      const refreshBuzz = completedSyncChanged(
        this.snapshot.sync.lastSuccess,
        snapshot.sync.lastSuccess,
      );
      this.snapshot = snapshot;
      if (refreshBuzz) this.invalidateBuzzTransactions();
      this.render();
      if (refreshBuzz && this.view === "buzz") void this.loadBuzzTransactions(false);
      if (soundId) void playSound(soundId).catch(() => undefined);
    });
    window.addEventListener("resize", () => this.fitBuzzStack());
    this.render();
  }

  private render(): void {
    this.captureScrollPosition();
    this.root.replaceChildren();
    const shell = element("main", "shell");
    shell.append(this.renderTitleBar());

    const content = element("section", "content");
    if (this.view === "notifications") content.append(this.renderNotifications());
    if (this.view === "buzz") content.append(this.renderBuzzTransactions());
    if (this.view === "settings") content.append(this.renderSettings());
    shell.append(content);

    this.root.append(shell);
    this.restoreScrollPosition();
    this.fitBuzzStack();
  }

  private fitBuzzStack(): void {
    const summary = this.root.querySelector<HTMLElement>(".account-summary");
    const stack = this.root.querySelector<HTMLElement>(".buzz-stack");
    if (!summary || !stack) return;
    const styles = window.getComputedStyle(summary);
    const inner = summary.clientWidth - parseFloat(styles.paddingLeft) - parseFloat(styles.paddingRight);
    const available = inner - BUZZ_HEADER_COLUMN_GAP - BUZZ_HEADER_MIN_PROFILE_WIDTH;
    fitBuzzFontSize(available, (fontPx) => {
      stack.style.setProperty("--buzz-font", `${fontPx}px`);
      return stack.getBoundingClientRect().width;
    });
  }

  private renderTitleBar(): HTMLElement {
    const titlebar = element("header", "titlebar");
    titlebar.setAttribute("data-tauri-drag-region", "");
    const brand = element("div", "titlebar-brand");
    brand.setAttribute("data-tauri-drag-region", "");
    const icon = element("img", "titlebar-icon");
    icon.src = appIcon;
    icon.alt = "";
    icon.draggable = false;
    icon.setAttribute("data-tauri-drag-region", "");
    const title = element("span", "", "Civitai Companion");
    title.setAttribute("data-tauri-drag-region", "");
    brand.append(icon, title);

    const actions = element("div", "titlebar-actions");
    const minimize = button("titlebar-button titlebar-minimize", "", () => {
      void getCurrentWindow().minimize().catch((error) => this.showNotice(this.errorMessage(error), true));
    });
    minimize.title = "Minimize";
    minimize.setAttribute("aria-label", "Minimize");
    const close = button("titlebar-button titlebar-close", "", () => {
      void getCurrentWindow().close().catch((error) => this.showNotice(this.errorMessage(error), true));
    });
    close.title = "Close";
    close.setAttribute("aria-label", "Close");
    actions.append(minimize, close);
    titlebar.append(brand, actions);
    return titlebar;
  }

  private captureScrollPosition(): void {
    const container = this.root.querySelector<HTMLElement>("[data-scroll-key]");
    const key = container?.dataset.scrollKey;
    if (!container || !key) return;

    const anchors = Array.from(container.querySelectorAll<HTMLElement>("[data-scroll-anchor]"));
    if (anchors.length === 0) return;

    const scrollTop = container.scrollTop;
    if (scrollTop <= 1) {
      this.scrollPositions.set(key, { scrollTop: 0, anchor: null, anchorOffset: 0 });
      return;
    }

    const containerTop = container.getBoundingClientRect().top;
    const anchor = anchors.find((candidate) => candidate.getBoundingClientRect().bottom > containerTop)
      ?? anchors.at(-1);
    this.scrollPositions.set(key, {
      scrollTop,
      anchor: anchor?.dataset.scrollAnchor ?? null,
      anchorOffset: anchor ? anchor.getBoundingClientRect().top - containerTop : 0,
    });
  }

  private restoreScrollPosition(): void {
    const container = this.root.querySelector<HTMLElement>("[data-scroll-key]");
    const key = container?.dataset.scrollKey;
    if (!container || !key) return;

    const position = this.scrollPositions.get(key);
    if (!position) return;
    container.scrollTop = position.scrollTop;
    if (!position.anchor) return;

    const anchor = Array.from(container.querySelectorAll<HTMLElement>("[data-scroll-anchor]"))
      .find((candidate) => candidate.dataset.scrollAnchor === position.anchor);
    if (!anchor) return;

    const currentOffset = anchor.getBoundingClientRect().top - container.getBoundingClientRect().top;
    container.scrollTop += currentOffset - position.anchorOffset;
  }

  private syncIndicatorClass(): string {
    if (this.snapshot.sync.updating) return "pending";
    if (!this.snapshot.credentialConfigured || this.snapshot.sync.error || !this.snapshot.sync.lastSuccess) return "failed";
    return "ready";
  }

  private renderNotifications(): HTMLElement {
    const view = element("div", "notifications-view");
    view.append(this.renderAccountSummary());

    const toolbar = element("nav", "filterbar");
    for (const item of FILTERS) {
      const control = button(`filter-button${this.filter === item.id ? " active" : ""}`, "", () => {
        this.filter = item.id;
        this.render();
      });
      control.title = item.label;
      control.setAttribute("aria-label", item.label);
      if (item.iconClass) control.append(element("span", `filter-symbol ${item.iconClass}`));
      else control.append(document.createTextNode(item.label));
      const count = this.filterUnreadCount(item.id);
      if (count > 0) control.append(element("span", "filter-count", count > 99 ? "99+" : String(count)));
      toolbar.append(control);
    }
    const markAll = button("mark-all", "", () => void this.run(() => backend.markAllRead()));
    markAll.title = "Mark all as read";
    markAll.setAttribute("aria-label", "Mark all as read");
    markAll.append(element("span", "icon-list-check"));
    markAll.disabled = this.busy || this.snapshot.notifications.unreadCount === 0;
    toolbar.append(markAll);
    const settings = button("settings-button", "", () => {
      this.view = "settings";
      this.settingsPage = "root";
      this.render();
    });
    settings.title = "Settings";
    settings.setAttribute("aria-label", "Settings");
    settings.append(element("span", "icon-settings"));
    toolbar.append(settings);
    view.append(toolbar);

    const list = element("div", "notification-list");
    list.dataset.scrollKey = `notifications:${this.filter}`;
    const filtered = this.snapshot.notifications.items.filter((item) => notificationMatchesFilter(item, this.filter));
    if (!this.snapshot.credentialConfigured) {
      list.append(this.emptyState("Configure an API key in Settings to connect your account."));
    } else if (filtered.length === 0) {
      list.append(this.emptyState("No notifications in this view."));
    } else {
      for (const item of filtered) list.append(this.renderNotification(item));
    }
    view.append(list);
    return view;
  }

  private renderAccountSummary(): HTMLElement {
    const summary = element("section", "account-summary");
    const identity = element("div", "profile-card");
    const avatar = button(`profile-avatar ${this.syncIndicatorClass()}`, "", () => void this.run(() => backend.sync()));
    avatar.title = "Sync now";
    avatar.setAttribute("aria-label", "Sync now");
    avatar.disabled = this.busy || this.snapshot.sync.updating;
    const fallback = element("span", "profile-fallback", this.snapshot.account.username.slice(0, 1).toUpperCase() || "?");
    avatar.append(fallback);
    if (isAllowedImageUrl(this.snapshot.account.profileImageUrl)) {
      const image = element("img");
      avatar.append(image);
      loadCivitaiImage(image, this.snapshot.account.profileImageUrl, () => image.remove());
    }
    avatar.append(element("span", "profile-refresh-icon"));
    const identityText = element("span", "profile-copy");
    if (this.snapshot.account.username) {
      const profileLink = button("profile-name-link", "", () => {
        void this.open(`https://civitai.red/user/${encodeURIComponent(this.snapshot.account.username)}`);
      });
      profileLink.title = "Go to profile";
      profileLink.append(
        element("strong", "profile-name", this.snapshot.account.username),
        element("span", "profile-name-hover", "Go to profile →"),
      );
      identityText.append(profileLink);
    } else {
      identityText.append(
        element("strong", "profile-name", "Not connected"),
        element("small", "profile-link", "Configure in Settings"),
      );
    }
    identity.append(avatar, identityText);
    summary.append(identity);

    const stats = element("div", "compact-stats");
    const followers = element("div", "stat-card follower-card");
    followers.title = this.snapshot.today.followersComplete
      ? "New followers today"
      : "New followers found today; notification history may be incomplete";
    followers.append(
      element("span", "icon-user"),
      element("span", "follower-plus", "+"),
      element("strong", "", String(this.snapshot.today.followers)),
      element("small", "", "Today"),
    );
    stats.append(followers);

    const balances = visibleBuzzBalances(this.snapshot.buzz);
    if (balances.length > 0) {
      const buzz = button(`stat-card buzz-stack buzz-count-${balances.length}`, "", () => {
        this.buzzAccountType = balances[0]?.kind ?? "blue";
        this.view = "buzz";
        this.render();
        void this.loadBuzzTransactions(false);
      });
      buzz.title = buzzBalanceTooltip(balances);
      buzz.setAttribute("aria-label", "Open Buzz transactions");
      balances.forEach((balance, index) => {
        if (index > 0) buzz.append(element("span", "buzz-separator"));
        buzz.append(this.statLine(formatCompactBuzz(balance.value), balance.kind));
      });
      buzz.append(element("span", "buzz-chevron"));
      stats.append(buzz);
    }
    summary.append(stats);
    return summary;
  }

  private statLine(value: string, className: string): HTMLElement {
    const line = element("div", `buzz-line ${className}`);
    line.append(element("span", "icon-bolt"), element("strong", "", value));
    return line;
  }

  private renderBuzzTransactions(): HTMLElement {
    const view = element("div", "subview buzz-ledger-view");
    view.append(this.viewHeading("Buzz transactions", "Previous and current month", () => {
      this.view = "notifications";
      this.render();
    }));

    const tabs = element("div", "buzz-account-tabs");
    tabs.setAttribute("role", "tablist");
    for (const accountType of ["blue", "yellow", "green"] as const) {
      const selected = accountType === this.buzzAccountType;
      const control = button(`buzz-account-tab ${accountType}${selected ? " active" : ""}`, "", () => {
        if (this.buzzAccountType === accountType) return;
        this.buzzAccountType = accountType;
        this.render();
        void this.loadBuzzTransactions(false);
      });
      control.setAttribute("role", "tab");
      control.setAttribute("aria-selected", String(selected));
      control.title = `${accountType[0]?.toUpperCase()}${accountType.slice(1)} Buzz`;
      control.setAttribute("aria-label", control.title);
      control.append(element("span", "icon-bolt"));
      tabs.append(control);
    }
    view.append(tabs);
    view.append(this.renderBuzzBalance());

    const state = this.buzzPages[this.buzzAccountType];
    const list = element("div", "buzz-transaction-list");
    list.dataset.scrollKey = `buzz:${this.buzzAccountType}`;
    if (state.error && state.items.length === 0) {
      const error = this.emptyState(state.error);
      error.classList.add("buzz-error");
      const retry = button("secondary", "Try again", () => void this.loadBuzzTransactions(false));
      error.append(retry);
      list.append(error);
    } else if (!state.loaded || (state.loading && state.items.length === 0)) {
      list.append(this.emptyState("Loading Buzz transactions…"));
    } else if (state.items.length === 0) {
      list.append(this.emptyState(`No ${this.buzzAccountType} Buzz transactions in this period.`));
    } else {
      for (const item of state.items) list.append(this.renderBuzzTransaction(item));
      if (state.error) list.append(element("p", "warning buzz-load-error", state.error));
      if (state.nextCursor) {
        const more = button("secondary full buzz-load-more", state.loading ? "Loading…" : "Load more", () => {
          void this.loadBuzzTransactions(true);
        });
        more.disabled = state.loading;
        list.append(more);
      }
    }
    view.append(list);
    return view;
  }

  private renderBuzzBalance(): HTMLElement {
    const kind = this.buzzAccountType;
    const row = element("div", `buzz-balance ${kind}`);
    row.append(element("span", "icon-bolt"));
    const copy = element("span", "buzz-balance-copy");
    copy.append(
      element("small", "", `${buzzKindLabel(kind)} Buzz balance`),
      element("strong", "", formatExactBuzz(this.snapshot.buzz[kind])),
    );
    row.append(copy);
    return row;
  }

  private renderBuzzTransaction(item: BuzzTransactionItem): HTMLElement {
    const card = element("article", "buzz-transaction");
    card.dataset.scrollAnchor = [item.date, item.kind, item.amount, item.description, item.imageUrl ?? ""].join("\u001f");
    const copy = element("div", "buzz-transaction-copy");
    const top = element("div", "buzz-transaction-top");
    const identity = element("span", "buzz-transaction-identity");
    identity.append(
      element("time", "", formatDate(item.date)),
      element("span", "buzz-transaction-kind", item.kind.toUpperCase()),
    );
    const amount = element("span", `buzz-transaction-amount ${item.accountType}`);
    amount.append(
      element("span", "icon-bolt"),
      element("strong", "", `${item.amount > 0 ? "+" : ""}${formatNumber(item.amount, 1)}`),
    );
    identity.append(amount);
    top.append(identity);
    copy.append(top, element("p", "", item.description));
    card.append(copy);

    if (item.imageUrl) {
      const preview = button("buzz-transaction-preview", "View image", () => void this.open(item.imageUrl!));
      preview.setAttribute("aria-label", "Open image on Civitai");
      if (item.thumbnailUrl && isAllowedImageUrl(item.thumbnailUrl)) {
        const image = element("img");
        image.loading = "lazy";
        preview.append(image);
        loadCivitaiImage(image, item.thumbnailUrl, () => image.remove());
      }
      card.append(preview);
    }
    return card;
  }

  private async loadBuzzTransactions(append: boolean): Promise<void> {
    const accountType = this.buzzAccountType;
    const state = this.buzzPages[accountType];
    if (state.loading || (!append && state.loaded)) return;
    if (append && !state.nextCursor) return;
    const revision = state.revision;
    state.loading = true;
    state.error = "";
    this.render();
    try {
      const page = await backend.buzzTransactions(accountType, append ? state.nextCursor : null);
      if (revision !== state.revision) return;
      state.items = append ? [...state.items, ...page.transactions] : page.transactions;
      state.nextCursor = page.nextCursor;
      state.loaded = true;
    } catch (error) {
      if (revision !== state.revision) return;
      state.error = this.errorMessage(error);
      state.loaded = true;
    } finally {
      state.loading = false;
      if (this.view === "buzz" && this.buzzAccountType === accountType) {
        this.render();
        if (!state.loaded) void this.loadBuzzTransactions(false);
      }
    }
  }

  private invalidateBuzzTransactions(): void {
    for (const state of Object.values(this.buzzPages)) {
      state.loaded = false;
      state.nextCursor = null;
      state.error = "";
      state.revision += 1;
    }
  }

  private filterUnreadCount(filter: Filter): number {
    if (filter === "all") return this.snapshot.notifications.unreadCount;
    return this.snapshot.notifications.items.filter(
      (item) => !item.read && notificationMatchesFilter(item, filter),
    ).length;
  }

  private renderNotification(item: NotificationItem): HTMLElement {
    const row = button(`notification${item.read ? "" : " unread"}`, "", () => {
      void this.openNotification(item);
    });
    row.dataset.scrollAnchor = item.id;
    const marker = element("span", `notification-icon ${this.notificationIconClass(item)}`);
    row.append(marker);

    const body = element("span", "notification-copy");
    const text = element("span", "notification-text");
    this.renderNotificationText(text, item);
    body.append(text);
    const meta = element("span", "notification-meta");
    meta.append(element("span", "notification-age", relativeTime(item.createdAt)));
    if (item.commentPreview) meta.append(element("span", "comment-preview", `· “${item.commentPreview}”`));
    body.append(meta);
    row.append(body);

    const isFollower = item.type.toLowerCase() === "followed-by";
    if (item.thumbnailUrl && isAllowedImageUrl(item.thumbnailUrl)) {
      const frame = element("span", `thumbnail-frame${item.type === "tip-received" ? " tip-thumbnail" : ""}${item.thumbnailKind === "avatar" || isFollower ? " avatar-thumbnail-frame" : ""}`);
      const image = element("img", "notification-thumbnail");
      image.loading = "lazy";
      if (item.thumbnailKind === "avatar" || isFollower) image.classList.add("avatar-thumbnail");
      frame.append(image);
      row.append(frame);
      loadCivitaiImage(image, item.thumbnailUrl, () => {
        if (isFollower) this.showFollowerFallback(frame);
        else frame.remove();
      });
    } else if (isFollower) {
      const frame = element("span", "thumbnail-frame avatar-thumbnail-frame");
      this.showFollowerFallback(frame);
      row.append(frame);
    }
    return row;
  }

  private showFollowerFallback(frame: HTMLElement): void {
    frame.classList.add("follower-avatar-fallback");
    frame.replaceChildren(element("span", "icon-user"));
  }

  private renderNotificationText(target: HTMLElement, item: NotificationItem): void {
    const value = item.text || item.type || "Civitai notification";
    const username = item.username?.trim() ?? "";
    const start = username ? value.toLocaleLowerCase().indexOf(username.toLocaleLowerCase()) : -1;
    if (start < 0) {
      target.textContent = value;
      return;
    }
    target.append(
      document.createTextNode(value.slice(0, start)),
      element("strong", "", value.slice(start, start + username.length)),
      document.createTextNode(value.slice(start + username.length)),
    );
  }

  private notificationIconClass(item: NotificationItem): string {
    const kind = item.type.toLowerCase();
    const category = item.category.toLowerCase();
    if (kind.startsWith("sticker-placement-") && (kind.endsWith("-pending") || kind.endsWith("-resolved"))) return "icon-sticker";
    if (kind === "collection-update") return "icon-collection";
    if (isModelUpdateNotification(item)) return "icon-model-grid";
    if (kind === "followed-by") return "icon-user";
    if (kind.includes("comment") || kind === "new-thread-response" || category === "comment") return "icon-comment";
    if (isSubmissionUpdateNotification(item)) return "icon-bell";
    if (kind.includes("reaction") || category === "milestone") return "icon-reaction";
    if (kind.includes("tip") || kind.includes("buzz") || category === "buzz") return "icon-bolt";
    return "icon-bell";
  }

  private async openNotification(item: NotificationItem): Promise<void> {
    if (!item.read) await this.run(() => backend.markRead(item.id));
    if (item.url) await this.open(item.url);
  }

  private renderSettings(): HTMLElement {
    const view = element("div", "subview settings-view");
    if (this.settingsPage === "root") {
      view.append(this.viewHeading("Settings", "Desktop preferences and security", () => {
        this.view = "notifications";
        this.render();
      }));
      const list = element("div", "settings-list");
      for (const page of SETTINGS_PAGES) {
        const entry = button("settings-entry", "", () => {
          this.settingsPage = page.id;
          this.render();
        });
        const copy = element("span");
        copy.append(element("strong", "", page.title), element("small", "", page.detail));
        entry.append(copy, element("span", "chevron", "›"));
        list.append(entry);
      }
      view.append(list);
      return view;
    }

    const selected = SETTINGS_PAGES.find((page) => page.id === this.settingsPage);
    view.append(this.viewHeading(selected?.title ?? "Settings", selected?.detail ?? "", () => {
      this.settingsPage = "root";
      this.render();
    }));
    if (this.settingsPage === "account") view.append(this.renderAccountSettings());
    if (this.settingsPage === "notifications") view.append(this.renderNotificationSettings());
    if (this.settingsPage === "behavior") view.append(this.renderBehaviorSettings());
    if (this.settingsPage === "privacy") view.append(this.renderPrivacySettings());
    if (this.settingsPage === "about") view.append(this.renderAbout());
    return view;
  }

  private renderAccountSettings(): HTMLElement {
    const panel = element("div", "settings-panel");
    const status = element("div", "credential-status");
    status.append(element("span", this.snapshot.credentialConfigured ? "status-dot ok" : "status-dot"));
    status.append(element("strong", "", this.snapshot.credentialConfigured ? "Credential configured" : "No credential configured"));
    panel.append(status);
    panel.append(element("p", "help", "The API key is stored in Windows Credential Manager. It is never displayed or returned to this interface."));

    const field = element("input", "text-input");
    field.type = "password";
    field.autocomplete = "new-password";
    field.placeholder = this.snapshot.credentialConfigured ? "Enter a replacement API key" : "Enter your Civitai API key";
    field.maxLength = 4096;
    field.setAttribute("aria-label", "Civitai API key");
    const save = button("primary", this.snapshot.credentialConfigured ? "Replace API key" : "Configure API key", () => {
      const value = field.value;
      field.value = "";
      void this.configureKey(value, field);
    });
    save.disabled = this.busy;
    const row = element("div", "button-row");
    row.append(save);
    if (this.snapshot.credentialConfigured) {
      row.append(button("secondary", "Test connection", () => void this.testConnection()));
      row.append(button("danger", "Remove", () => void this.confirmAction(
        "Remove the API key and clear account data from this app?",
        "Remove",
        () => backend.removeKey(),
      )));
    }
    panel.append(field, row);
    panel.append(element("h3", "section-title", "Permission Inspector"));
    panel.append(this.renderPermissions(this.snapshot.permissions));
    return panel;
  }

  private async configureKey(value: string, field: HTMLInputElement): Promise<void> {
    if (!value.trim()) {
      this.showNotice("Enter an API key first.", true);
      field.focus();
      return;
    }
    try {
      await this.run(() => backend.configureKey(value));
    } finally {
      field.value = "";
      value = "";
    }
  }

  private async testConnection(): Promise<void> {
    await this.run(async () => {
      await backend.testConnection();
      return backend.snapshot();
    });
  }

  private renderPermissions(permissions: PermissionStatus[]): HTMLElement {
    const list = element("div", "permission-list");
    for (const permission of permissions) {
      const item = element("div", "permission-item");
      const copy = element("span");
      copy.append(element("strong", "", permission.label), element("small", "", permission.description));
      item.append(copy, element("span", permission.granted ? "permission granted" : "permission denied", permission.granted ? "Granted" : "Missing"));
      list.append(item);
    }
    return list;
  }

  private renderNotificationSettings(): HTMLElement {
    const panel = element("div", "settings-panel");
    panel.append(this.toggleRow(
      "Windows notifications",
      "Show new Civitai events through the Windows notification center.",
      this.snapshot.settings.windowsNotificationsEnabled,
      (checked) => void this.saveSettings({ windowsNotificationsEnabled: checked }),
    ));
    panel.append(element("h3", "section-title", "Event sounds"));
    panel.append(this.soundRow("Site activity", "siteActivity"));
    panel.append(this.soundRow("Buzz tips", "tips"));
    panel.append(this.soundRow("New followers", "followers"));
    return panel;
  }

  private soundRow(label: string, key: keyof NotificationSounds): HTMLElement {
    const row = element("label", "sound-row");
    row.append(element("span", "", label));
    const controls = element("span", "sound-controls");
    const select = element("select", "select-input");
    const options: ReadonlyArray<[string, string]> = [["", "Off"], ["1", "Sound 1"], ["2", "Sound 2"], ["3", "Sound 3"], ["4", "Sound 4"]];
    for (const [value, text] of options) {
      const option = element("option", "", text);
      option.value = value;
      option.selected = (this.snapshot.settings.notificationSounds[key] ?? "") === value;
      select.append(option);
    }
    select.addEventListener("change", () => {
      const sounds = { ...this.snapshot.settings.notificationSounds, [key]: select.value || null };
      void this.saveSettings({ notificationSounds: sounds as NotificationSounds });
    });
    const preview = button("mini-button", "Play", () => {
      const id = select.value as SoundId;
      if (id) void this.previewSound(id);
    });
    preview.disabled = !select.value;
    select.addEventListener("change", () => { preview.disabled = !select.value; });
    controls.append(select, preview);
    row.append(controls);
    return row;
  }

  private async previewSound(id: SoundId): Promise<void> {
    try {
      const fallback = await backend.previewSound(id);
      if (fallback) await playSound(fallback);
    } catch (error) {
      this.showNotice(this.errorMessage(error), true);
    }
  }

  private renderBehaviorSettings(): HTMLElement {
    const panel = element("div", "settings-panel");
    panel.append(this.toggleRow(
      "Thread responses",
      "Include supported response notifications from followed discussions.",
      this.snapshot.settings.threadResponsesEnabled,
      (checked) => void this.saveSettings({ threadResponsesEnabled: checked }),
    ));
    panel.append(this.toggleRow(
      "Close to tray",
      "Keep the app running in the notification area when the window is closed.",
      this.snapshot.settings.closeToTray,
      (checked) => void this.saveSettings({ closeToTray: checked }),
    ));
    panel.append(this.toggleRow(
      "Start with Windows",
      "Launch minimized with Windows. Disabled by default.",
      this.snapshot.settings.startWithWindows,
      (checked) => void this.saveSettings({ startWithWindows: checked }),
    ));
    const polling = element("label", "field-row");
    const copy = element("span");
    copy.append(element("strong", "", "Polling interval"), element("small", "", "Between 1 and 60 minutes"));
    const input = element("input", "number-input");
    input.type = "number";
    input.min = "1";
    input.max = "60";
    input.step = "1";
    input.value = String(this.snapshot.settings.pollingMinutes);
    input.addEventListener("change", () => {
      const value = Number(input.value);
      if (Number.isFinite(value) && value >= 1 && value <= 60) {
        void this.saveSettings({ pollingMinutes: value });
      } else {
        input.value = String(this.snapshot.settings.pollingMinutes);
        this.showNotice("Polling must be between 1 and 60 minutes.", true);
      }
    });
    polling.append(copy, input);
    panel.append(polling);
    return panel;
  }

  private toggleRow(title: string, detail: string, checked: boolean, onChange: (checked: boolean) => void): HTMLElement {
    const row = element("label", "toggle-row");
    const copy = element("span");
    copy.append(element("strong", "", title), element("small", "", detail));
    const input = element("input");
    input.type = "checkbox";
    input.checked = checked;
    input.addEventListener("change", () => onChange(input.checked));
    const toggle = element("span", "toggle");
    row.append(copy, input, toggle);
    return row;
  }

  private renderPrivacySettings(): HTMLElement {
    const panel = element("div", "settings-panel");
    panel.append(element("p", "help", "Backups contain preferences only. Credentials, account details and notifications are excluded. Import never changes Start with Windows."));
    const actions = element("div", "stacked-actions");
    actions.append(button("secondary full", "Export preferences", () => void this.exportPreferences()));
    const importButton = button("secondary full", "Import preferences", () => this.chooseImport());
    actions.append(importButton);
    actions.append(button("secondary full", "Reset cached data", () => void this.confirmAction(
      "Clear notifications and account cache while keeping the API key?",
      "Reset cache",
      () => backend.resetCache(),
    )));
    actions.append(button("danger full", "Clear account data", () => void this.confirmAction(
      "Remove the credential and all locally cached account data?",
      "Clear data",
      () => backend.clearAccount(),
    )));
    panel.append(actions);
    return panel;
  }

  private async exportPreferences(): Promise<void> {
    try {
      const path = await backend.exportPreferences();
      await this.dialog("Preferences exported", `Saved to:\n${path}`, "OK");
    } catch (error) {
      this.showNotice(this.errorMessage(error), true);
    }
  }

  private chooseImport(): void {
    const input = element("input");
    input.type = "file";
    input.accept = "application/json,.json";
    input.addEventListener("change", () => {
      const file = input.files?.[0];
      if (!file) return;
      if (file.size > 1024 * 1024) {
        this.showNotice("The preference file exceeds 1 MiB.", true);
        return;
      }
      void file.text()
        .then((json) => this.run(() => backend.importPreferences(json)))
        .catch((error: unknown) => this.showNotice(this.errorMessage(error), true));
    }, { once: true });
    input.click();
  }

  private renderAbout(): HTMLElement {
    const panel = element("div", "settings-panel about-panel");
    const mark = element("img", "about-mark");
    mark.src = appIcon;
    mark.alt = "Civitai Companion app icon";
    panel.append(mark);
    panel.append(element("h2", "", "Civitai Companion"));
    panel.append(element("p", "version", `Version ${this.snapshot.appVersion}`));
    const credit = button("about-credit", "", () => void this.open("https://civitai.red/user/Tar_Tarj"));
    credit.title = "Open Tar_Tarj on Civitai";
    const avatar = element("span", "about-credit-avatar");
    avatar.append(element("span", "icon-user"));
    if (isAllowedImageUrl(this.snapshot.account.profileImageUrl)) {
      const image = element("img");
      avatar.append(image);
      loadCivitaiImage(image, this.snapshot.account.profileImageUrl, () => image.remove());
    }
    const copy = element("span", "about-credit-copy");
    copy.append(element("small", "", "Created by"), element("strong", "", "Tar_Tarj"));
    credit.append(avatar, copy);
    panel.append(credit);
    panel.append(element("p", "help", "A local-only Windows companion. No telemetry, analytics, browser injection, remote companion server or automatic updater is included."));
    panel.append(element("p", "help", "Authenticated network traffic is restricted to approved Civitai HTTPS endpoints. The interface receives state and permission information, never the API key."));
    return panel;
  }

  private viewHeading(title: string, subtitle: string, back: () => void): HTMLElement {
    const heading = element("div", "view-heading");
    heading.append(iconButton("Back", "‹", back));
    const copy = element("div");
    copy.append(element("h1", "", title), element("p", "", subtitle));
    heading.append(copy);
    return heading;
  }

  private emptyState(message: string): HTMLElement {
    const empty = element("div", "empty");
    empty.append(element("strong", "", "Nothing to show"), element("p", "", message));
    return empty;
  }

  private async saveSettings(patch: Parameters<typeof backend.updateSettings>[0]): Promise<void> {
    await this.run(() => backend.updateSettings(patch));
  }

  private async open(url: string): Promise<void> {
    try {
      await backend.openCivitaiUrl(url);
    } catch (error) {
      this.showNotice(this.errorMessage(error), true);
    }
  }

  private async run(
    action: () => Promise<AppSnapshot>,
  ): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    this.render();
    try {
      this.snapshot = await action();
    } catch (error) {
      this.showNotice(this.errorMessage(error), true);
    } finally {
      this.busy = false;
      this.render();
    }
  }

  private showNotice(message: string, isError = false): void {
    if (isError) void this.dialog("Unable to complete action", message, "OK");
  }

  private async confirmAction(
    message: string,
    confirmLabel: string,
    action: () => Promise<AppSnapshot>,
  ): Promise<void> {
    if (await this.dialog("Confirm action", message, confirmLabel, "Cancel")) {
      await this.run(action);
    }
  }

  private dialog(title: string, message: string, confirmLabel: string, cancelLabel?: string): Promise<boolean> {
    return new Promise((resolve) => {
      const dialog = element("dialog", "app-dialog");
      const form = element("form", "app-dialog-panel");
      form.method = "dialog";
      form.append(element("h2", "", title), element("p", "", message));
      const actions = element("div", "app-dialog-actions");
      if (cancelLabel) {
        const cancel = element("button", "secondary", cancelLabel);
        cancel.type = "submit";
        cancel.value = "cancel";
        actions.append(cancel);
      }
      const confirm = element("button", cancelLabel ? "danger" : "primary", confirmLabel);
      confirm.type = "submit";
      confirm.value = "confirm";
      actions.append(confirm);
      form.append(actions);
      dialog.append(form);
      dialog.addEventListener("close", () => {
        resolve(dialog.returnValue === "confirm");
        dialog.remove();
      }, { once: true });
      document.body.append(dialog);
      dialog.showModal();
    });
  }

  private errorMessage(error: unknown): string {
    return typeof error === "string" && error.length <= 240 ? error : "The operation could not be completed.";
  }
}
