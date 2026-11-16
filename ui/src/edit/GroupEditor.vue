<template>
	<form
		class="ts-group-editor"
		@submit.prevent="save"
		@keydown.esc="cancel">
		<h3 class="ts-group-editor__title">
			{{ propertyLabel }}
		</h3>
		<ol class="ts-group-editor__rows">
			<li v-for="( row, i ) in rows" :key="row.key">
				<StatementEditor
					:row="row"
					:position="i + 1"
					:property-label="propertyLabel"
					:data="data"
					:api="api"
					:labels="labels"
					:disabled="saving"
				/>
			</li>
		</ol>
		<div class="ts-group-editor__add">
			<CdxButton
				type="button"
				:disabled="saving"
				@click="add">
				{{ t( 'ts-edit-add-value' ) }}
			</CdxButton>
		</div>
		<CdxMessage v-if="problem" type="error">
			{{ problem }}
		</CdxMessage>
		<div class="ts-group-editor__actions">
			<CdxButton
				type="submit"
				action="progressive"
				weight="primary"
				:disabled="saving"
			>
				{{ saving ? t( 'ts-edit-saving' ) : t( 'ts-edit-save' ) }}
			</CdxButton>
			<CdxButton
				v-if="conflict"
				type="button"
				:disabled="saving"
				@click="emit( 'restart' )"
			>
				{{ t( 'ts-edit-reload' ) }}
			</CdxButton>
			<CdxButton
				type="button"
				:disabled="saving"
				@click="cancel">
				{{ t( 'ts-edit-cancel' ) }}
			</CdxButton>
		</div>
	</form>
</template>

<script setup lang="ts">
// A statement group's editor (ADR 0003 §8): every value of one property, with its rank,
// qualifiers and sources, and new values, saved together as one wbeditentity with
// baserevid. The group keeps its order; removed values go with the same revision.
import { onMounted, reactive, ref } from 'vue';
import { CdxButton, CdxMessage } from '@wikimedia/codex';
import { ApiError } from './api';
import type { Api } from './api';
import { msg } from './data';
import type { EditData } from './data';
import StatementEditor from './StatementEditor.vue';
import { claimsEdit, newRow, rowOf } from './statements';
import type { Statement } from './statements';

const props = defineProps<{
	data: EditData;
	api: Api;
	conceptBase: string;
	property: string;
	propertyLabel: string;
	datatype: string;
	statements: Statement[];
	labels: Record<string, string>;
	datatypes: Record<string, string>;
	lastrevid: number;
}>();

const emit = defineEmits<{
	saved: [];
	cancel: [];
	restart: [];
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );
const cx = { conceptBase: props.conceptBase, datatypes: props.datatypes };
const labels = reactive( { ...props.labels } );
const rows = reactive( props.statements.map( ( s ) => rowOf( s, cx ) ) );
if ( rows.length === 0 ) {
	rows.push( newRow( props.property, props.datatype ) );
}
const saving = ref( false );
const problem = ref( '' );
const conflict = ref( false );

onMounted( () => {
	document.querySelector<HTMLElement>( '.ts-group-editor input, .ts-group-editor button' )?.focus();
} );

function add(): void {
	rows.push( newRow( props.property, props.datatype ) );
}

function cancel(): void {
	if ( !saving.value ) {
		emit( 'cancel' );
	}
}

async function save(): Promise<void> {
	problem.value = '';
	conflict.value = false;
	const edit = claimsEdit( rows, cx );
	if ( 'problem' in edit ) {
		const n = rows.findIndex( ( r ) => r.key === edit.key ) + 1;
		problem.value = `${ props.propertyLabel } ${ n }: ${ t( edit.problem ) }`;
		return;
	}
	if ( !edit.changed ) {
		emit( 'cancel' );
		return;
	}
	saving.value = true;
	try {
		await props.api.write( {
			action: 'wbeditentity',
			id: props.data.id,
			baserevid: props.lastrevid,
			data: JSON.stringify( { claims: edit.claims } )
		} );
		emit( 'saved' );
	} catch ( e ) {
		if ( e instanceof ApiError && e.code === 'editconflict' ) {
			conflict.value = true;
			problem.value = t( 'ts-edit-conflict' );
		} else {
			problem.value = t( 'ts-edit-failed', e instanceof Error ? e.message : String( e ) );
		}
	} finally {
		saving.value = false;
	}
}
</script>
