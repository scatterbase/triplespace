<template>
	<div class="ts-sheet-create ts-sheet-file">
		<CdxMessage v-if="problem" type="error">
			{{ problem }}
		</CdxMessage>
		<p v-if="sending" class="ts-sheet__summary">
			{{ sending }}
		</p>
		<MapColumns
			v-if="mapping"
			:data="data"
			:api="api"
			:sheet="mapping"
			@apply="mapped"
			@cancel="mapping = null"
		/>
	</div>
</template>

<script setup lang="ts">
// The file of Special:CreateEntitiesFromFile (ADR 0106 §7, A6, A9; 21 §6.8). A chosen
// file is read here and never uploaded: its columns are mapped, and the table that
// comes of it is posted as text to Special:CreateManyEntities, whose form opens holding
// it. A draft with columns and no rows is a saved mapping: a file of its shape goes
// under its columns without the step. A file of QuickStatements commands is not a
// table: the page's own form sends it, and the server reads it as
// Special:QuickStatements does.
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { CdxMessage } from '@wikimedia/codex';
import type { Api } from '../edit/api';
import MapColumns from './MapColumns.vue';
import { msg } from './data';
import type { FileData } from './data';
import { readFile } from './readers';
import type { Sheet as FileSheet } from './readers';
import { column, textOf } from './text';

const props = defineProps<{
	data: FileData;
	api: Api;
	form: HTMLFormElement;
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );

const file = props.form.elements.namedItem( 'file' ) as HTMLInputElement | null;
const mapping = ref<FileSheet | null>( null );
const problem = ref( '' );
const sending = ref( '' );

// Posts the table to the page that edits it.
function open( rows: string[][], reference: string[] ): void {
	const doc = props.form.ownerDocument;
	const post = doc.createElement( 'form' );
	post.method = 'post';
	post.action = props.data.open;
	post.hidden = true;
	const fields: [ string, string ][] = [
		[ 'wpStep', 'open' ],
		[ 'rows', textOf( rows, false ) ],
		[ 'reference', reference.join( '\n' ) ],
		[ 'draft', props.data.draft ]
	];
	for ( const [ name, value ] of fields ) {
		const input = doc.createElement( 'input' );
		input.type = 'hidden';
		input.name = name;
		input.value = value;
		post.append( input );
	}
	doc.body.append( post );
	post.submit();
}

function mapped( rows: string[][], reference: string[] ): void {
	mapping.value = null;
	sending.value = t( 'ts-sheet-file-opening', String( Math.max( rows.length - 1, 0 ) ) );
	open( rows, reference );
}

async function chosen(): Promise<void> {
	const upload = file?.files?.[ 0 ];
	if ( !upload || !file ) {
		return;
	}
	problem.value = '';
	mapping.value = null;
	try {
		const sheet = await readFile( upload, props.data.lang );
		if ( sheet.kind === 'quickstatements' ) {
			// Commands, not a table: the form sends the file, and the server reads it.
			sending.value = t( 'ts-sheet-file-commands' );
			props.form.submit();
			return;
		}
		file.value = '';
		const kept = ( props.data.columns ?? [] ).filter( ( h ) => h.toLowerCase() !== 'id' );
		const known = kept.some( ( h ) => column( h, props.data.lang ).kind !== 'plain' );
		if ( known && kept.length === sheet.headers.length ) {
			// A saved mapping: a draft with columns and no rows, and a file of its shape.
			sending.value = t( 'ts-sheet-map-saved', String( sheet.rows.length ) );
			open( [ kept, ...sheet.rows ], [] );
			return;
		}
		mapping.value = sheet;
	} catch ( e ) {
		file.value = '';
		problem.value = t( 'ts-sheet-file-failed', e instanceof Error ? e.message : String( e ) );
	}
}

const onChosen = (): void => {
	chosen().catch( () => {} );
};

// The file is read when it is chosen, so the form's own button is not needed.
const button = props.form.querySelector<HTMLElement>( 'button[type="submit"]' );

onMounted( () => {
	file?.addEventListener( 'change', onChosen );
	if ( button ) {
		button.hidden = true;
	}
} );

onBeforeUnmount( () => {
	file?.removeEventListener( 'change', onChosen );
	if ( button ) {
		button.hidden = false;
	}
} );
</script>
