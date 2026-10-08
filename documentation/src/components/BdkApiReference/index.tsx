import {
  Fragment,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import clsx from "clsx";

import CodeBlock from "~/components/CodeBlock";
import Link from "~/components/Link";
import apiData from "~/data/bdk-api.json";
import { useHistory, useLocation } from "~/utils/router";

import { LANGUAGES, type Language, type LanguageId } from "./languages";
import styles from "./styles.module.css";


type BdkParam = {
  name: string;
  type: string;
  default: string | null;
  docs: string;
};

type BdkFunc = {
  name: string;
  docs: string;
  params: BdkParam[];
  returns: string | null;
  throws: string | null;
  isAsync: boolean;
};

type BdkVariant = {
  name: string;
  fields: BdkParam[];
};

type BdkItemKind =
  | "object"
  | "callback"
  | "record"
  | "enum"
  | "error";

type BdkItem = {
  name: string;
  kind: BdkItemKind;
  docs: string;
  fields: BdkParam[];
  variants: BdkVariant[];
  methods: BdkFunc[];
};

type BdkApiDoc = {
  version: string;
  docVersion: string;
  source: string;
  functions: BdkFunc[];
  items: BdkItem[];
};

type BdkApiData = {
  versions: BdkApiDoc[];
};

type Selection = {
  docVersion: string;
  languageId: LanguageId;
};

type GroupedItems = {
  kind: BdkItemKind;
  items: BdkItem[];
};


function firstOrThrow<T>(
  values: readonly T[],
  message: string,
): T {
  const value = values.at(0);

  if (value === undefined) {
    throw new Error(message);
  }

  return value;
}


const VERSIONS: BdkApiDoc[] = (apiData as BdkApiData).versions;

const DEFAULT_VERSION_ENTRY: BdkApiDoc = firstOrThrow(
  VERSIONS,
  "BDK API data does not contain any versions",
);

const DEFAULT_LANGUAGE_ENTRY: Language = firstOrThrow(
  LANGUAGES,
  "BDK API reference does not define any languages",
);

const DEFAULT_VERSION = DEFAULT_VERSION_ENTRY.docVersion;
const DEFAULT_LANGUAGE = DEFAULT_LANGUAGE_ENTRY.id;

const VERSION_PARAM = "version";
const LANGUAGE_PARAM = "language";

const ITEM_KIND_ORDER: readonly BdkItemKind[] = [
  "object",
  "callback",
  "record",
  "enum",
  "error",
];

const KIND_LABELS: Record<BdkItemKind, string> = {
  object: "Class",
  callback: "Interface",
  record: "Data type",
  enum: "Enum",
  error: "Error",
};

const KIND_HEADINGS: Record<BdkItemKind, string> = {
  object: "Classes",
  callback: "Interfaces",
  record: "Data types",
  enum: "Enums",
  error: "Errors",
};


const isKnownVersion = (
  docVersion: string | null,
): docVersion is string =>
  docVersion !== null
  && VERSIONS.some(
    (entry) => entry.docVersion === docVersion,
  );

const isKnownLanguage = (
  languageId: string | null,
): languageId is LanguageId =>
  languageId !== null
  && LANGUAGES.some(
    (entry) => entry.id === languageId,
  );


const readSelection = (
  search: string,
): Selection => {
  const params = new URLSearchParams(search);

  const requestedVersion = params.get(VERSION_PARAM);
  const requestedLanguage = params.get(LANGUAGE_PARAM);

  return {
    docVersion: isKnownVersion(requestedVersion)
      ? requestedVersion
      : DEFAULT_VERSION,
    languageId: isKnownLanguage(requestedLanguage)
      ? requestedLanguage
      : DEFAULT_LANGUAGE,
  };
};


const selectionSearch = (
  selection: Selection,
  search: string,
): string => {
  const params = new URLSearchParams(search);

  params.set(
    VERSION_PARAM,
    selection.docVersion,
  );

  params.set(
    LANGUAGE_PARAM,
    selection.languageId,
  );

  return `?${params.toString()}`;
};


const normalizedSearch = (
  search: string,
): string => {
  const value = new URLSearchParams(search).toString();

  return value
    ? `?${value}`
    : "";
};


const SelectionSearchContext = createContext(
  selectionSearch(
    {
      docVersion: DEFAULT_VERSION,
      languageId: DEFAULT_LANGUAGE,
    },
    "",
  ),
);


const slug = (
  ...parts: string[]
): string =>
  parts
    .join("-")
    .replace(
      /[^a-zA-Z0-9]+/g,
      "-",
    )
    .replace(
      /^-+|-+$/g,
      "",
    )
    .toLowerCase();


const funcAnchor = (
  func: BdkFunc,
  owner?: string,
): string =>
  slug(
    owner ?? "fn",
    func.name,
  );


const itemAnchor = (
  item: BdkItem,
): string =>
  slug(item.name);


const memberAnchor = (
  ownerAnchor: string,
  kind: string,
  name: string,
): string =>
  slug(
    ownerAnchor,
    kind,
    name,
  );


function anchorsForFunc(
  func: BdkFunc,
  owner?: string,
): string[] {
  const anchor = funcAnchor(
    func,
    owner,
  );

  return [
    anchor,
    ...func.params.map(
      (param) =>
        memberAnchor(
          anchor,
          "param",
          param.name,
        ),
    ),
  ];
}


function anchorsForDoc(
  doc: BdkApiDoc,
): string[] {
  const anchors: string[] = [
    "functions",
  ];

  for (const func of doc.functions) {
    anchors.push(
      ...anchorsForFunc(func),
    );
  }

  for (const item of doc.items) {
    const anchor = itemAnchor(item);

    anchors.push(
      anchor,
      slug(
        item.kind,
        "types",
      ),
    );

    for (const field of item.fields) {
      anchors.push(
        memberAnchor(
          anchor,
          "field",
          field.name,
        ),
      );
    }

    for (const variant of item.variants) {
      anchors.push(
        memberAnchor(
          anchor,
          "variant",
          variant.name,
        ),
      );
    }

    for (const method of item.methods) {
      anchors.push(
        ...anchorsForFunc(
          method,
          item.name,
        ),
      );
    }
  }

  return anchors;
}


function signature(
  func: BdkFunc,
  language: Language,
  owner?: string,
): string {
  const params = func.params
    .map(
      (param) => {
        const type = language.type(
          param.type,
        );

        const suffix =
          param.default === null
            ? ""
            : ` = ${language.default(param.default)}`;

        if (language.id === "rust") {
          return `${param.name}: ${type}${suffix}`;
        }

        return `${language.field(param.name)}: ${type}${suffix}`;
      },
    )
    .join(", ");

  const name = language.func(
    func.name,
  );

  const returns =
    func.returns === null
      ? null
      : language.type(func.returns);

  const prefix = owner
    ? `${owner}.`
    : "";

  if (language.id === "rust") {
    const asyncKeyword =
      func.isAsync
        ? "async "
        : "";

    const result =
      func.throws !== null
        ? `Result<${returns ?? "()"}, ${func.throws}>`
        : returns;

    return (
      `${asyncKeyword}fn ${prefix}${name}(${params})`
      + (
        result
          ? ` -> ${result}`
          : ""
      )
    );
  }

  if (language.id === "python") {
    const asyncKeyword =
      func.isAsync
        ? "async "
        : "";

    return (
      `${asyncKeyword}def ${prefix}${name}(${params})`
      + (
        returns
          ? ` -> ${returns}`
          : ""
      )
    );
  }

  const suspend =
    func.isAsync
      ? "suspend "
      : "";

  const throwsAnnotation =
    func.throws === null
      ? ""
      : `@Throws(${language.errorType(func.throws)}::class)\n`;

  return (
    `${throwsAnnotation}${suspend}fun ${prefix}${name}(${params})`
    + (
      returns
        ? `: ${returns}`
        : ""
    )
  );
}


type HashLinkProps = {
  anchor: string;
  label: string;
};

function HashLink({
  anchor,
  label,
}: HashLinkProps) {
  const search = useContext(
    SelectionSearchContext,
  );

  const title =
    `Direct link to ${label}`;

  return (
    <Link
      className="hash-link"
      to={`${search}#${anchor}`}
      aria-label={title}
      title={title}
      translate="no"
    >
      &#8203;
    </Link>
  );
}


type AnchoredProps = {
  as: "h2" | "h3" | "h4" | "td";
  anchor: string;
  label: string;
  className?: string;
  children: ReactNode;
};

function Anchored({
  as: As,
  anchor,
  label,
  className,
  children,
}: AnchoredProps) {
  return (
    <As
      id={anchor}
      className={clsx(
        "anchor",
        className,
      )}
    >
      {children}
      <HashLink
        anchor={anchor}
        label={label}
      />
    </As>
  );
}


type ParamTableProps = {
  rows: BdkParam[];
  language: Language;
  caption: string;
  ownerAnchor: string;
  rowKind: string;
};

function ParamTable({
  rows,
  language,
  caption,
  ownerAnchor,
  rowKind,
}: ParamTableProps) {
  if (rows.length === 0) {
    return null;
  }

  const hasDefaults = rows.some(
    (row) => row.default !== null,
  );

  return (
    <table className={styles.table}>
      <thead>
        <tr>
          <th>{caption}</th>
          <th>Type</th>
          {hasDefaults && <th>Default</th>}
          <th>Description</th>
        </tr>
      </thead>

      <tbody>
        {rows.map(
          (row) => {
            const name = language.field(
              row.name,
            );

            return (
              <tr key={row.name}>
                <Anchored
                  as="td"
                  anchor={memberAnchor(
                    ownerAnchor,
                    rowKind,
                    row.name,
                  )}
                  label={name}
                  className={styles.nameCell}
                >
                  <code>{name}</code>
                </Anchored>

                <td>
                  <code>
                    {language.type(row.type)}
                  </code>
                </td>

                {hasDefaults && (
                  <td>
                    {row.default === null
                      ? "—"
                      : (
                        <code>
                          {language.default(
                            row.default,
                          )}
                        </code>
                      )}
                  </td>
                )}

                <td>
                  {row.docs || "—"}
                </td>
              </tr>
            );
          },
        )}
      </tbody>
    </table>
  );
}


type FuncEntryProps = {
  func: BdkFunc;
  language: Language;
  owner?: string;
};

function FuncEntry({
  func,
  language,
  owner,
}: FuncEntryProps) {
  const anchor = funcAnchor(
    func,
    owner,
  );

  const name = language.func(
    func.name,
  );

  return (
    <div className={styles.entry}>
      <Anchored
        as="h4"
        anchor={anchor}
        label={name}
        className={styles.entryTitle}
      >
        <code>{name}</code>
      </Anchored>

      {func.docs && (
        <p>{func.docs}</p>
      )}

      <CodeBlock
        language={language.prism}
      >
        {signature(
          func,
          language,
          owner,
        )}
      </CodeBlock>

      <ParamTable
        rows={func.params}
        language={language}
        caption="Parameter"
        ownerAnchor={anchor}
        rowKind="param"
      />

      {func.throws !== null && (
        <p className={styles.meta}>
          Raises{" "}
          <code>
            {language.errorType(
              func.throws,
            )}
          </code>
        </p>
      )}
    </div>
  );
}


type ItemEntryProps = {
  item: BdkItem;
  language: Language;
};

function ItemEntry({
  item,
  language,
}: ItemEntryProps) {
  const dataCarrying =
    item.variants.some(
      (variant) =>
        variant.fields.length > 0,
    );

  const anchor = itemAnchor(item);

  const name =
    item.kind === "error"
      ? language.errorType(item.name)
      : item.name;

  return (
    <section className={styles.item}>
      <Anchored
        as="h3"
        anchor={anchor}
        label={name}
        className={styles.itemTitle}
      >
        <code>{name}</code>

        <span className={styles.badge}>
          {KIND_LABELS[item.kind]}
        </span>
      </Anchored>

      {item.docs && (
        <p>{item.docs}</p>
      )}

      <ParamTable
        rows={item.fields}
        language={language}
        caption="Field"
        ownerAnchor={anchor}
        rowKind="field"
      />

      {item.variants.length > 0 && (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>
                {item.kind === "error"
                  ? "Variant"
                  : "Case"}
              </th>

              <th>
                Associated data
              </th>
            </tr>
          </thead>

          <tbody>
            {item.variants.map(
              (variant) => {
                const variantName =
                  item.kind === "error"
                  && language.id === "kotlin"
                    ? `${language.errorType(item.name)}.${variant.name}`
                    : language.variant(
                      variant.name,
                      dataCarrying,
                    );

                return (
                  <tr key={variant.name}>
                    <Anchored
                      as="td"
                      anchor={memberAnchor(
                        anchor,
                        "variant",
                        variant.name,
                      )}
                      label={variantName}
                      className={styles.nameCell}
                    >
                      <code>
                        {variantName}
                      </code>
                    </Anchored>

                    <td>
                      {variant.fields.length === 0
                        ? "—"
                        : variant.fields.map(
                          (field) => (
                            <div key={field.name}>
                              <code>
                                {language.field(field.name)}
                                {": "}
                                {language.type(field.type)}
                              </code>
                            </div>
                          ),
                        )}
                    </td>
                  </tr>
                );
              },
            )}
          </tbody>
        </table>
      )}

      {item.methods.map(
        (method) => (
          <FuncEntry
            key={method.name}
            func={method}
            language={language}
            owner={item.name}
          />
        ),
      )}
    </section>
  );
}


export default function BdkApiReference() {
  const [
    selection,
    setSelection,
  ] = useState<Selection>(
    () => ({
      docVersion: DEFAULT_VERSION,
      languageId: DEFAULT_LANGUAGE,
    }),
  );

  const {
    docVersion,
    languageId,
  } = selection;

  const history = useHistory();
  const location = useLocation();

  useEffect(
    () => {
      const requested = readSelection(
        location.search,
      );

      setSelection(requested);

      const canonical = selectionSearch(
        requested,
        location.search,
      );

      if (
        canonical
        !== normalizedSearch(location.search)
      ) {
        history.replace({
          search: canonical,
          hash: location.hash,
        });
      }
    },
    [
      history,
      location.hash,
      location.search,
    ],
  );

  const select = useCallback(
    (
      next: Partial<Selection>,
    ) => {
      const nextSelection: Selection = {
        ...selection,
        ...next,
      };

      history.replace({
        search: selectionSearch(
          nextSelection,
          location.search,
        ),
        hash: location.hash,
      });
    },
    [
      history,
      location.hash,
      location.search,
      selection,
    ],
  );

  const language = useMemo<Language>(
    () =>
      LANGUAGES.find(
        (entry) =>
          entry.id === languageId,
      )
      ?? DEFAULT_LANGUAGE_ENTRY,
    [
      languageId,
    ],
  );

  const doc = useMemo<BdkApiDoc>(
    () =>
      VERSIONS.find(
        (entry) =>
          entry.docVersion === docVersion,
      )
      ?? DEFAULT_VERSION_ENTRY,
    [
      docVersion,
    ],
  );

  const anchors = useMemo<Set<string>>(
    () =>
      new Set(
        anchorsForDoc(doc),
      ),
    [
      doc,
    ],
  );

  useEffect(
    () => {
      const rawAnchor =
        location.hash.slice(1);

      if (!rawAnchor) {
        return;
      }

      const anchor =
        decodeURIComponent(rawAnchor);

      if (!anchors.has(anchor)) {
        return;
      }

      document
        .getElementById(anchor)
        ?.scrollIntoView();
    },
    [
      anchors,
      location.hash,
    ],
  );

  const grouped = useMemo<GroupedItems[]>(
    () =>
      ITEM_KIND_ORDER
        .map(
          (kind): GroupedItems => ({
            kind,
            items: doc.items.filter(
              (item) =>
                item.kind === kind,
            ),
          }),
        )
        .filter(
          (group) =>
            group.items.length > 0,
        ),
    [
      doc,
    ],
  );

  return (
    <SelectionSearchContext.Provider
      value={selectionSearch(
        selection,
        location.search,
      )}
    >
      <div>
        <div className={styles.toolbar}>
          <div
            className={styles.tabs}
            role="tablist"
            aria-label="SDK language"
          >
            {LANGUAGES.map(
              (entry) => (
                <button
                  key={entry.id}
                  type="button"
                  role="tab"
                  aria-selected={
                    entry.id
                    === languageId
                  }
                  className={
                    entry.id === languageId
                      ? styles.tabActive
                      : styles.tab
                  }
                  onClick={
                    () =>
                      select({
                        languageId: entry.id,
                      })
                  }
                >
                  {entry.label}
                </button>
              ),
            )}
          </div>

          <label className={styles.version}>
            Version

            <select
              value={docVersion}
              onChange={
                (event) =>
                  select({
                    docVersion:
                      event.target.value,
                  })
              }
              aria-label="SDK version"
            >
              {VERSIONS.map(
                (entry) => (
                  <option
                    key={entry.docVersion}
                    value={entry.docVersion}
                  >
                    {entry.docVersion}.x
                  </option>
                ),
              )}
            </select>
          </label>
        </div>

        <p className={styles.meta}>
          Generated from{" "}
          <code>{doc.source}</code>
          {" "}at{" "}
          <code>
            bcaip-sdk {doc.version}
          </code>.
        </p>

        <Anchored
          as="h2"
          anchor="functions"
          label="Functions"
        >
          Functions
        </Anchored>

        {doc.functions.map(
          (func) => (
            <FuncEntry
              key={func.name}
              func={func}
              language={language}
            />
          ),
        )}

        {grouped.map(
          (group) => (
            <Fragment key={group.kind}>
              <Anchored
                as="h2"
                anchor={slug(
                  group.kind,
                  "types",
                )}
                label={
                  KIND_HEADINGS[
                    group.kind
                  ]
                }
              >
                {
                  KIND_HEADINGS[
                    group.kind
                  ]
                }
              </Anchored>

              {group.items.map(
                (item) => (
                  <ItemEntry
                    key={item.name}
                    item={item}
                    language={language}
                  />
                ),
              )}
            </Fragment>
          ),
        )}
      </div>
    </SelectionSearchContext.Provider>
  );
}