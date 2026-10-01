import React, {type ReactNode} from 'react';
import clsx from 'clsx';
import useBaseUrl from '@docusaurus/useBaseUrl';
import {
  PageMetadata,
  HtmlClassNameProvider,
  ThemeClassNames,
} from '@docusaurus/theme-common';
import BlogLayout from '@theme/BlogLayout';
import BlogListPaginator from '@theme/BlogListPaginator';
import SearchMetadata from '@theme/SearchMetadata';
import type {Props} from '@theme/BlogListPage';
import BlogListPageStructuredData from '@theme/BlogListPage/StructuredData';
import styles from './styles.module.css';

function BlogListPageMetadata(props: Props): ReactNode {
  const {metadata} = props;

  return (
    <>
      <PageMetadata
        title="BCAIP Blog"
        description="News, engineering notes, architecture decisions, and technical articles from BCAIP."
      />
      <SearchMetadata tag="blog_posts_list" />
    </>
  );
}

const getAuthorName = (author: any): string =>
  typeof author === 'string'
    ? author
    : (author.name || author.key || author);

function AuthorDisplay({authors}: {authors: any[]}) {
  if (!authors?.length) {
    return null;
  }

  return (
    <div className={styles.postAuthors}>
      {authors.slice(0, 3).map((author, index) => (
        <span key={index} className={styles.authorName}>
          {getAuthorName(author)}
        </span>
      ))}
    </div>
  );
}

function BlogPostCard({post}: {post: any}) {
  const url = useBaseUrl(post.content.metadata.permalink);

  const imageUrl = post.content.frontMatter.image
    ? useBaseUrl(post.content.frontMatter.image)
    : null;

  const title = post.content.metadata.title;
  const formattedDate = post.content.metadata.formattedDate;
  const description =
    post.content.metadata.description ||
    post.content.frontMatter.description;

  const authors =
    post.content?.metadata?.authors ||
    post.content?.frontMatter?.authors ||
    [];

  return (
    <article className={styles.postCard}>
      {imageUrl && (
        <a href={url} className={styles.postImage}>
          <img src={imageUrl} alt="" />
        </a>
      )}

      <div className={styles.postContent}>
        <div className={styles.postDate}>{formattedDate}</div>

        <h2 className={styles.postTitle}>
          <a href={url}>{title}</a>
        </h2>

        <AuthorDisplay authors={authors} />

        {description && (
          <p className={styles.postDescription}>{description}</p>
        )}

        <a href={url} className={styles.readMore}>
          Read article →
        </a>
      </div>
    </article>
  );
}

function BlogListPageContent(props: Props): ReactNode {
  const {metadata, items} = props;
  const isFirstPage = !metadata.permalink.includes('/page/');

  const validItems = items.filter(
    (item) =>
      item.content?.metadata?.title &&
      item.content?.frontMatter,
  );

  return (
    <BlogLayout sidebar={undefined}>
      <main className={styles.blogContainer}>
        {isFirstPage && (
          <header className={styles.hero}>
            <div className={styles.heroEyebrow}>BezotCorp</div>

            <h1 className={styles.heroTitle}>BCAIP Blog</h1>

            <p className={styles.heroDescription}>
              News, engineering notes, architecture decisions, and technical
              articles from the development of BCAIP.
            </p>
          </header>
        )}

        {validItems.length > 0 ? (
          <div className={styles.postsGrid}>
            {validItems.map((post, index) => (
              <BlogPostCard key={index} post={post} />
            ))}
          </div>
        ) : (
          isFirstPage && (
            <section className={styles.emptyState}>
              <h2>The BCAIP story starts here.</h2>
              <p>
                Releases, engineering work, architecture decisions, and
                technical articles will be published here.
              </p>
            </section>
          )
        )}

        <div className={styles.paginationWrapper}>
          <BlogListPaginator metadata={metadata} />
        </div>
      </main>
    </BlogLayout>
  );
}

export default function BlogListPage(props: Props): ReactNode {
  return (
    <HtmlClassNameProvider
      className={clsx(
        ThemeClassNames.wrapper.blogPages,
        ThemeClassNames.page.blogListPage,
      )}
    >
      <BlogListPageMetadata {...props} />
      <BlogListPageStructuredData {...props} />
      <BlogListPageContent {...props} />
    </HtmlClassNameProvider>
  );
}
